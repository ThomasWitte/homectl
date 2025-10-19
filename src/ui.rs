use eframe::egui::{Button, Color32, Pos2, Rect, Stroke};
use eframe::{CreationContext, egui};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tokio::sync::mpsc::channel;
use tokio_util::sync::CancellationToken;

use crate::data::{
    create_rooms, save_rooms_to_file, update_actors, update_rooms, HeatingState, Room, SensorHistoryItem
};

pub struct MyApp {
    ct: CancellationToken,
    rooms: Arc<Mutex<Vec<Room>>>,
}

impl MyApp {
    pub fn new(cc: &CreationContext) -> Self {
        let rooms = Arc::new(Mutex::new(create_rooms()));

        let rt = Runtime::new().expect("Unable to create Runtime");
        let ct = CancellationToken::new();

        // Enter the runtime so that `tokio::spawn` is available immediately.
        let _enter = rt.enter();

        // Execute the runtime in its own thread.
        // The future doesn't have to do anything. In this example, it just sleeps
        // forever.
        let ct_clone = ct.clone();
        let ctx_clone = cc.egui_ctx.clone();
        let rooms_clone = rooms.clone();
        std::thread::spawn(move || {
            rt.block_on(async {
                let (tx, rx) = channel(10);
                let handle = tokio::spawn(crate::bt::bt_main(tx));
                let update_rooms_handle = tokio::spawn(update_rooms(rx, rooms_clone.clone(), ctx_clone));
                let update_actors_handle = tokio::spawn(update_actors(rooms_clone.clone()));

                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {
                        println!("Ctrl-C received, shutting down");
                        ct_clone.cancel();
                    }
                    _ = ct_clone.cancelled() => {
                        println!("Cancellation requested, shutting down");
                    }
                    res = handle => {
                        println!("shutdown bt");
                        if let Err(err) = res {
                            eprintln!("Error: {}", err);
                        }
                    }
                    res = update_rooms_handle => {
                        println!("shutdown rooms");
                        if let Err(err) = res {
                            eprintln!("Error in update_rooms: {}", err);
                        }
                    }
                    res = update_actors_handle => {
                        println!("shutdown actors");
                        if let Err(err) = res {
                            eprintln!("Error in update_actors: {}", err);
                        }
                    }
                }
            })
        });

        Self { ct, rooms }
    }
}

impl eframe::App for MyApp {
    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(300)
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.ct.is_cancelled() {
            println!("Application is exiting, closing window.");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        let mut rooms = self.rooms.lock().unwrap();
        let history_len = Duration::from_secs(24 * 60 * 60);

        egui::CentralPanel::default().show(ctx, |ui| {
            let row_height = 480.0 / rooms.len() as f32;
            let row_width = 800.0;
            let margin = row_height / 20.0;
            let mut pos = 0.0;
            for room in &mut *rooms {
                let col = if let Some(sensor) = &room.sensor {
                    if sensor.temperature < 16.0 {
                        Color32::from_rgb(0, 0, 255)
                    } else if sensor.temperature > 26.0 {
                        Color32::from_rgb(255, 0, 0)
                    } else if sensor.temperature > 21.0 {
                        Color32::from_rgb(
                            ((sensor.temperature - 21.0) / 5.0 * 255.0) as u8,
                            ((1.0 - (sensor.temperature - 21.0) / 5.0) * 255.0) as u8,
                            0,
                        )
                    } else {
                        Color32::from_rgb(
                            0,
                            ((1.0 - (21.0 - sensor.temperature) / 5.0) * 255.0) as u8,
                            ((21.0 - sensor.temperature) / 5.0 * 255.0) as u8,
                        )
                    }
                } else {
                    Color32::from_rgb(200, 200, 200)
                };
                ui.painter().rect(
                    Rect::everything_below(pos).with_max_y(pos + row_height),
                    0,
                    col,
                    Stroke {
                        width: 1.0,
                        color: Color32::BLACK,
                    },
                    egui::StrokeKind::Middle,
                );
                ui.painter().text(
                    egui::pos2(2.0 * margin, pos + 2.0 * margin),
                    egui::Align2::LEFT_TOP,
                    &room.name,
                    egui::FontId::proportional(row_height / 4.0),
                    Color32::BLACK,
                );
                if let Some(sensor) = &room.sensor {
                    ui.painter().text(
                        egui::pos2(2.0 * margin, pos + row_height / 2.0 + 2.0 * margin),
                        egui::Align2::LEFT_TOP,
                        format!("{:.1}°C ({}s) {}%", sensor.temperature, (room.sensor_ttl.unwrap_or(Instant::now()) - Instant::now()).as_secs(), sensor.humidity),
                        egui::FontId::proportional(row_height / 4.0),
                        Color32::BLACK,
                    );

                    let x_min = row_width / 6.0 * 2.0;
                    let x_max = row_width - 3.5 * row_height - margin;

                    let y_min = pos + margin;
                    let y_max = pos + row_height - margin;

                    ui.painter().rect(
                        Rect::from_two_pos(
                            Pos2 { x: x_min, y: y_min },
                            Pos2 { x: x_max, y: y_max },
                        ),
                        0,
                        Color32::WHITE,
                        Stroke {
                            width: 1.0,
                            color: Color32::BLACK,
                        },
                        egui::StrokeKind::Middle,
                    );

                    let width = x_max - x_min;
                    let height = y_max - y_min;

                    let max_temp = 23.0;
                    let min_temp = 17.0;

                    for SensorHistoryItem { data, timestamp } in room
                        .sensor_history
                        .iter()
                        .enumerate()
                        .filter_map(|(i, item)| if i % 10 == 0 { Some(item) } else { None })
                    {
                        if data.temperature < min_temp || data.temperature > max_temp {
                            continue;
                        }
                        let x = x_max
                            - width / history_len.as_secs() as f32
                                * (Instant::now() - *timestamp).as_secs() as f32;
                        let y =
                            y_max - (data.temperature - min_temp) / (max_temp - min_temp) * height;
                        ui.painter()
                            .circle_filled(Pos2 { x, y }, 1.0, Color32::BLUE);
                    }
                }
                if let Some(actor) = &mut room.actor {
                    let buttons_pos = row_width - 3.5 * row_height;
                    ui.put(
                        Rect::from_two_pos(
                            Pos2 {
                                x: buttons_pos,
                                y: pos + margin / 2.0,
                            },
                            Pos2 {
                                x: buttons_pos + row_height - margin,
                                y: pos + (row_height - margin) / 2.0,
                            },
                        ),
                        Button::new("Auto"),
                    );
                    ui.put(
                        Rect::from_two_pos(
                            Pos2 {
                                x: buttons_pos + row_height * 2.5,
                                y: pos + margin / 2.0,
                            },
                            Pos2 {
                                x: buttons_pos + row_height * 3.0 - margin,
                                y: pos + (row_height - margin) / 2.0,
                            },
                        ),
                        Button::new("⬆"),
                    );
                    ui.put(
                        Rect::from_two_pos(
                            Pos2 {
                                x: buttons_pos + row_height * 3.0,
                                y: pos + margin / 2.0,
                            },
                            Pos2 {
                                x: buttons_pos + row_height * 3.5 - margin,
                                y: pos + (row_height - margin) / 2.0,
                            },
                        ),
                        Button::new("⬇"),
                    );
                    for i in 0..=6 {
                        let btn = if let HeatingState::Manual(level) = &actor.state {
                            if *level == i {
                                Button::new(format!("{}", i)).selected(true)
                            } else {
                                Button::new(format!("{}", i))
                            }
                        } else {
                            Button::new(format!("{}", i))
                        };
                        if ui.put(
                            Rect::from_two_pos(
                                Pos2 {
                                    x: (buttons_pos as i32 + i as i32 * row_height as i32 / 2)
                                        as f32,
                                    y: pos + (row_height + margin) / 2.0,
                                },
                                Pos2 {
                                    x: ((buttons_pos + row_height / 2.0 - margin) as i32
                                        + i as i32 * row_height as i32 / 2)
                                        as f32,
                                    y: pos + row_height - margin / 2.0,
                                },
                            ),
                            btn,
                        ).clicked() {
                            actor.state = HeatingState::Manual(i);
                        };
                    }
                }
                pos += row_height;
            }
        });
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        save_rooms_to_file(&*self.rooms.lock().unwrap(), "rooms.json");
        println!("State saved.");
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.ct.cancel();
        println!("Exiting application.");
    }
}
