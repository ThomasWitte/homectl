use bluer::{
    Adapter, AdapterEvent, Address, Device, DiscoveryFilter, DiscoveryTransport,
    gatt::remote::Characteristic,
};
use futures::{StreamExt, pin_mut, stream::SelectAll};
use std::{collections::HashSet, env};
use tokio::sync::mpsc::Sender;

use crate::data::TPSensorData;

async fn query_device(adapter: &Adapter, addr: Address) -> bluer::Result<Option<Characteristic>> {
    let device = adapter.device(addr)?;
    let name = device.name().await?;
    if name.is_some() && name.unwrap().starts_with("TP357") {
        return query_tp(&device).await;
    }
    Ok(None)
}

async fn query_tp(device: &Device) -> bluer::Result<Option<Characteristic>> {
    println!("TP found!");

    if !device.is_connected().await? {
        println!("    Connecting...");
        let mut retries = 2;
        loop {
            match device.connect().await {
                Ok(()) => break,
                Err(err) if retries > 0 => {
                    println!("    Connect error: {}", &err);
                    retries -= 1;
                }
                Err(err) => return Err(err),
            }
        }
        println!("    Connected");
    } else {
        println!("    Already connected");
    }

    println!("    Enumerating services...");
    for service in device.services().await? {
        let uuid = service.uuid().await?;
        println!("    Service UUID: {}", &uuid);
        println!("    Service data: {:?}", service.all_properties().await?);
        for char in service.characteristics().await? {
            let uuid = char.uuid().await?;
            println!("    Characteristic UUID: {}", &uuid);
            println!(
                "    Characteristic data: {:?}",
                char.all_properties().await?
            );
            if uuid == uuid::Uuid::from_u128(0x000102030405060708090a0b0c0d2b10) {
                println!("characteristic found");
                return Ok(Some(char));
            }
        }
    }

    Ok(None)
}

pub async fn bt_main(tx: Sender<TPSensorData>) -> bluer::Result<()> {
    let with_changes = env::args().any(|arg| arg == "--changes");
    let le_only = env::args().any(|arg| arg == "--le");
    let br_edr_only = env::args().any(|arg| arg == "--bredr");
    let filter_addr: HashSet<_> = env::args()
        .filter_map(|arg| arg.parse::<Address>().ok())
        .collect();

    env_logger::init();
    let session = bluer::Session::new().await?;
    println!("Adapters: {:?}", session.adapter_names().await?);

    let adapter = session.adapter("hci1")?;
    println!(
        "Discovering devices using Bluetooth adapter {}\n",
        adapter.name()
    );
    adapter.set_powered(true).await?;

    let filter = DiscoveryFilter {
        transport: if le_only {
            DiscoveryTransport::Le
        } else if br_edr_only {
            DiscoveryTransport::BrEdr
        } else {
            DiscoveryTransport::Auto
        },
        ..Default::default()
    };
    adapter.set_discovery_filter(filter).await?;
    println!(
        "Using discovery filter:\n{:#?}\n\n",
        adapter.discovery_filter().await
    );

    let device_events = adapter.discover_devices().await?;
    pin_mut!(device_events);

    let mut all_change_events = SelectAll::new();

    loop {
        tokio::select! {
            Some(device_event) = device_events.next() => {
                match device_event {
                    AdapterEvent::DeviceAdded(addr) => {
                        if !filter_addr.is_empty() && !filter_addr.contains(&addr) {
                            continue;
                        }

                        let res = query_device(&adapter, addr).await;
                        if let Ok(Some(ref c)) = res {
                            let tx = tx.clone();
                            let c = c.clone();
                            tokio::spawn(async move {
                                let mut reader = c.notify_io().await.expect("notify failed");
                                loop {
                                    match reader.recv().await {
                                        Ok(data) => {
                                            if data.len() < 6 {
                                                continue;
                                            }
                                            let temp = (data[3] as i32 + data[4] as i32 * 256) as f32 / 10.0;
                                            let humidity = data[5] as u8;
                                            tx.send(TPSensorData {
                                                address: addr.to_string(),
                                                temperature: temp,
                                                humidity,
                                            }).await.expect("Failed to send sensor data");
                                        },
                                        Err(e) => {
                                            // try to reconnect
                                            eprintln!("error from notify stream: {e:?}");
                                            reader = c.notify_io().await.expect("notify failed");
                                        },
                                    }
                                }
                            });
                        }
                        if let Err(err) = res {
                            println!("    Error: {}", &err);
                        }

                        if with_changes {
                            let device = adapter.device(addr)?;
                            let change_events = device.events().await?.map(move |evt| (addr, evt));
                            all_change_events.push(change_events);
                        }
                    }
                    _ => (),
                }
            }
            else => break,
        }
    }

    Ok(())
}
