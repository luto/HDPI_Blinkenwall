#![allow(dead_code)]
#![allow(unused)]

#[macro_use] extern crate log;
extern crate env_logger;

#[macro_use]
extern crate glium;
extern crate chrono;
use glium::glutin;
use glium::DisplayBuild;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate serde_json;

mod shadertoy;
use shadertoy::ShaderToy;

mod server;
extern crate git2;
extern crate uuid;
mod database;

extern crate mpv;
mod video;
use video::Video;

use std::sync::mpsc;
use std::process;

extern crate portaudio;
extern crate atomic_ring_buffer;
extern crate rustfft;

mod config;

// https://www.shadertoy.com/view/XssczX
// https://www.shadertoy.com/view/XlfGzH

enum ActiveView {
    Off,
    ShaderToy,
    Video,
    Emulator,
    VNC
}

fn main() {
    env_logger::init().unwrap();
    let config = match config::Config::new("blinkenwall.json") {
        Err(err) => {
            error!("Error in config file: {}", err);
            process::exit(-1);
        }
        Ok(config) => config
    };
    let mut database = database::Database::new(&config.database.repository);
    let display = glutin::WindowBuilder::new()
        .with_depth_buffer(24)
        .with_fullscreen(glutin::get_primary_monitor())
        .with_vsync()
        .build_glium()
        .unwrap();
    let window = display.get_window().unwrap();
    window.set_inner_size(config.display.width, config.display.height);

    let (server_thread, command_receiver) = server::open_server(&config.server.address, config.server.port);
    let mut video = Video::new(&window);
    let mut shadertoy : Option<ShaderToy> = None;

    let mut active_view = ActiveView::Off;

    loop {
        match active_view {
            ActiveView::Off => {},
            ActiveView::ShaderToy => {
                if let Some(ref mut s) = shadertoy {
                    s.step(&display);
                }
            },
            ActiveView::Video =>
                match video.step(&window) {
                    None => {},
                    Some(evt) => info!("MPV event: {:?}", evt),
                },
            ActiveView::Emulator => {},
            ActiveView::VNC => {},
        }
        match command_receiver.try_recv() {
            Ok(message) => {
                let (cmd, resp) = message;
                match cmd {
                    server::Command::ListShaders => resp.send_list(database.list().unwrap()),
                    server::Command::ReadShader(id) =>
                        match database.read(&id) {
                            Ok(shader) => resp.send_shader(&shader),
                            Err(error) => resp.send_error(400, &format!("{}", error))
                        },
                    server::Command::WriteShader(_, _) => resp.send_error(404, "Not implemented"),
                    server::Command::CreateShader(shader) =>
                        match database.add(&shader, &format!("Add shader for {}", resp.address())) {
                            Ok(id) => resp.send_id(&id),
                            Err(error) => resp.send_error(400, &format!("{}", error))
                        },
                    server::Command::RemoveShader(id) =>
                        match database.remove(&id, &format!("Remove shader for {}", resp.address())) {
                            Ok(_) => resp.send_ok(),
                            Err(error) => resp.send_error(400, &format!("{}", error))
                        },
                    server::Command::ActivateShader(id) => {
                        info!("[{}] Activating shader {}", resp.address(), id);
                        match database.read(&id) {
                            Ok(shader) => {
                                match active_view {
                                    ActiveView::Video => video.stop(),
                                    _ => {}
                                }

                                active_view = ActiveView::ShaderToy;
                                shadertoy = Some(ShaderToy::new_with_audio(&display, &shader.source));
                                resp.send_ok()
                            },
                            Err(error) => {
                                resp.send_error(404, &error.message())
                            }
                        }
                    },
                    server::Command::PlayVideo(url) => {
                        active_view = ActiveView::Video;
                        video.play(&url);
                        shadertoy = None;
                        resp.send_ok()
                    },
                    server::Command::StopVideo => {
                        match active_view {
                            ActiveView::Video => {
                                active_view = ActiveView::Off;
                                video.stop();
                            },
                            _ => {}
                        }
                        resp.send_ok()
                    },
                }.unwrap();
            },
            Err(err) => match err {
                mpsc::TryRecvError::Empty => (),
                mpsc::TryRecvError::Disconnected => break,
            }
        }
    }

    let _ = server_thread.join().unwrap();
}
