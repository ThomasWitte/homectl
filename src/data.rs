use eframe::egui::Context;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::Receiver;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TPSensorData {
    pub address: String,
    pub temperature: f32,
    pub humidity: u8,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub enum HeatingState {
    Manual(u8), // power level 0-6
    Auto(f32),  // target temperature
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct HeatingActor {
    pub address: String,
    pub state: HeatingState,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Room {
    pub name: String,
    pub sensor_address: String,
    #[serde(skip)]
    pub sensor_ttl: Option<std::time::Instant>,
    pub sensor: Option<TPSensorData>,
    pub sensor_history: Vec<SensorHistoryItem>,
    pub actor: Option<HeatingActor>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct SensorHistoryItem {
    pub data: TPSensorData,
    #[serde(with = "approx_instant")]
    pub timestamp: std::time::Instant,
}

mod approx_instant {
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};
    use std::time::{Instant, SystemTime};

    pub fn serialize<S>(instant: &Instant, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let system_now = SystemTime::now();
        let instant_now = Instant::now();
        let approx = system_now - (instant_now - *instant);
        approx.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Instant, D::Error>
    where
        D: Deserializer<'de>,
    {
        let de = SystemTime::deserialize(deserializer)?;
        let system_now = SystemTime::now();
        let instant_now = Instant::now();
        let duration = system_now.duration_since(de).map_err(Error::custom)?;
        let approx = instant_now - duration;
        Ok(approx)
    }
}

pub fn create_rooms() -> Vec<Room> {
    let history = std::fs::File::open("rooms.json");
    if let Ok(file) = history {
        let reader = std::io::BufReader::new(file);
        if let Ok(rooms) = serde_json::from_reader(reader) {
            return rooms;
        }
    }

    vec![
        Room {
            name: "Galerie".to_string(),
            sensor_address: "10:76:36:76:66:1E".to_string(),
            sensor_ttl: None,
            sensor: None,
            sensor_history: Vec::new(),
            actor: None,
        },
        Room {
            name: "Schlafzimmer".to_string(),
            sensor_address: "D1:D7:3F:67:8C:EF".to_string(),
            sensor_ttl: None,
            sensor: None,
            sensor_history: Vec::new(),
            actor: Some(HeatingActor {
                address: "http://shellypro3-ece334ed1928.local/relay/2".to_string(),
                state: HeatingState::Manual(3),
            }),
        },
        // Room {
        //     name: "Bad oben".to_string(),
        //     sensor_address: "".to_string(),
        //     sensor: None,
        //     sensor_ttl: None,
        //     actor: None,
        // },
        Room {
            name: "Kinderzimmer".to_string(),
            sensor_address: "D2:7C:11:BC:05:E3".to_string(),
            sensor: None,
            sensor_history: Vec::new(),
            sensor_ttl: None,
            actor: None,
        },
        // Room {
        //     name: "Gäste-WC".to_string(),
        //     sensor_address: "".to_string(),
        //     sensor: None,
        //     sensor_ttl: None,
        //     actor: None,
        // },
        Room {
            name: "Küche/Diele".to_string(),
            sensor_address: "C9:B5:08:81:6A:AC".to_string(),
            sensor: None,
            sensor_history: Vec::new(),
            sensor_ttl: None,
            actor: None,
        },
        Room {
            name: "Wohnzimmer".to_string(),
            sensor_address: "FA:74:A7:99:89:04".to_string(),
            sensor: None,
            sensor_history: Vec::new(),
            sensor_ttl: None,
            actor: None,
        },
        // Room {
        //     name: "Bad unten".to_string(),
        //     sensor_address: "".to_string(),
        //     sensor: None,
        //     sensor_ttl: None,
        //     actor: None,
        // },
        Room {
            name: "Bäckerei".to_string(),
            sensor_address: "10:76:36:C2:B7:87".to_string(),
            sensor_ttl: None,
            sensor: None,
            sensor_history: Vec::new(),
            actor: None,
        },
    ]
}

pub async fn update_actors(
    rooms: Arc<Mutex<Vec<Room>>>,
) {
    println!("Starting update_actors loop");
    let client = reqwest::ClientBuilder::new().build().unwrap();
    loop {
        let mut requests = Vec::new();
        if let Ok(rooms) = rooms.lock() {
            for room in &*rooms {
                if let Some(actor) = &room.actor {
                    println!("found actor!");
                    match actor.state {
                        HeatingState::Manual(level) => {
                            let time = level as u32 * 3600/6;
                            let url = &actor.address;
                            let query = [("turn", "on"), ("timer", &format!("{time}"))];
                            let request = client.get(url).query(&query);
                            println!("Sending request: {request:?}");
                            requests.push(request.send());
                        },
                        HeatingState::Auto(_) => unimplemented!()
                    }
                }
            }
        }
        for request in requests {
            match request.await {
                Ok(response) => println!("{response:?}"),
                Err(e) => eprintln!("{e}")
            }
        }
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}

pub async fn update_rooms(
    mut rx: Receiver<TPSensorData>,
    rooms: Arc<Mutex<Vec<Room>>>,
    ctx: Context,
) {
    loop {
        let sensor = rx.recv().await;

        let sensor = match sensor {
            Some(s) => s,
            None => continue,
        };
        let mut rooms = rooms.lock().unwrap();

        // update rooms list with new sensor data
        let history_len = Duration::from_secs(24 * 60 * 60);

        if let Some(existing) = rooms
            .iter_mut()
            .find(|s| s.sensor_address == sensor.address)
        {
            existing.sensor = Some(sensor.clone());
            existing.sensor_history.push(SensorHistoryItem {
                data: sensor,
                timestamp: Instant::now(),
            });
            while existing.sensor_history[0].timestamp < Instant::now() - history_len {
                existing.sensor_history.remove(0);
            }
            existing.sensor_ttl = Some(Instant::now() + std::time::Duration::from_secs(300));
        } else {
            rooms.push(Room {
                name: sensor.address.clone(),
                sensor_address: sensor.address.clone(),
                sensor_ttl: Some(Instant::now() + std::time::Duration::from_secs(300)),
                sensor: Some(sensor),
                sensor_history: vec![],
                actor: None,
            });
        }

        // Remove stale sensors
        for room in &mut *rooms {
            if let Some(ttl) = room.sensor_ttl {
                if Instant::now() > ttl {
                    room.sensor = None;
                    room.sensor_ttl = None;
                }
            }
        }
        ctx.request_repaint();
    }
}

pub fn save_rooms_to_file(rooms: &Vec<Room>, path: &str) {
    let history_file = std::fs::File::create(path).unwrap();
    let mut history_writer = std::io::BufWriter::new(history_file);
    serde_json::to_writer(&mut history_writer, &rooms).unwrap();
}
