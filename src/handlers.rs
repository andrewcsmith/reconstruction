extern crate rusty_machine;
extern crate soundsym;
extern crate portaudio;
extern crate bounded_spsc_queue;
extern crate conrod;
extern crate find_folder;
extern crate piston_window;

use soundsym::*;
use portaudio::{Continue, DuplexStreamCallbackArgs, DuplexStreamSettings, PortAudio, StreamParameters, DeviceIndex};
use bounded_spsc_queue::{Producer, Consumer};
use crossbeam::sync::SegQueue;
use rusty_machine::prelude::*;

use std::borrow::Cow;
use std::sync::{Mutex, Arc};
use std::rc::Rc;
use std::cell::RefCell;
use std::mem::transmute;
use std::{thread, time};

use super::*;

// Import relevant structs
use piston_window::{PistonWindow, Window, AdvancedWindow, G2d, G2dTexture, Texture, TextureSettings, WindowSettings};
use piston_window::texture::UpdateTexture;
use conrod::backend::piston::event::{convert, UpdateEvent};
use conrod::{color, widget, Colorable, Positionable, Sizeable, Widget, Labelable};

// Set window dimensions
const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;

const BLOCK_SIZE: usize = 64;
const DEFAULT_THRESHOLD: usize = 5;
const DEFAULT_DEPTH: usize = 4;

widget_ids! {
    pub struct Ids { 
        canvas, 
        plot, 
        reconstruct_button, 
        play_button, 
        launch_audio_button,
        stop_audio_button,
        in_devices_list,
        out_devices_list,
        analyze_sound_button,
        threshold_box, 
        depth_box,
        audio_device,
    }
}

struct ReconstructionApp {
    threshold_text: String,
    depth_text: String,
    devices: Option<Vec<(DeviceIndex, String)>>,
    in_device: Option<usize>,
    out_device: Option<usize>,
    window: PistonWindow,
}

impl ReconstructionApp {
    pub fn new<T>() -> Result<ReconstructionApp, Error<T>> {
        // instantiate window
        let mut window: PistonWindow = try!(WindowSettings::new("Reconstruction", [WIDTH, HEIGHT])
                          .samples(4)
                          .decorated(true)
                          .exit_on_esc(true)
                          .build());

        window.set_position([0, 0]);

        Ok(ReconstructionApp {
            threshold_text: String::new(),
            depth_text: String::new(),
            devices: None,
            in_device: None,
            out_device: None,
            window: window,
        })
    }
}

pub fn gui_handler<'a, T>(audio_commands_producer: Producer<AudioHandlerEvent>, dictionary_commands_producer: mpsc::Sender<DictionaryHandlerEvent>, gui_recv: mpsc::Receiver<GuiHandlerEvent>) -> Result<(), Error<T>> {
    let mut app = try!(ReconstructionApp::new());
    let mut ui = conrod::UiBuilder::new([WIDTH as f64, HEIGHT as f64]).build();
    let ids = Ids::new(ui.widget_id_generator());

    let assets = find_folder::Search::KidsThenParents(3, 5).for_folder("assets").unwrap();

    try!(ui.fonts.insert_from_file(assets.join("LH-Line1-Sans-Thin.ttf")));

    let mut text_vertex_data = Vec::new();

    let mut glyph_cache = {
        const SCALE_TOLERANCE: f32 = 0.1;
        const POSITION_TOLERANCE: f32 = 0.1;
        let cache = conrod::text::GlyphCache::new(WIDTH, HEIGHT, SCALE_TOLERANCE, POSITION_TOLERANCE);
        cache
    };

    let mut text_texture_cache = {
        let buffer_len = WIDTH as usize * HEIGHT as usize;
        let init = vec![128; buffer_len];
        let settings = TextureSettings::new();
        let factory = &mut app.window.factory;
        let texture = G2dTexture::from_memory_alpha(factory, &init, WIDTH, HEIGHT, &settings).unwrap();
        texture
    };
        
    let image_map = conrod::image::Map::new();

    while let Some(event) = app.window.next() {
        while let Ok(handler_event) = gui_recv.try_recv() {
            match handler_event {
                GuiHandlerEvent::Devices(d) => app.devices = Some(d),
                GuiHandlerEvent::InDevice(d) => app.in_device = Some(d),
                GuiHandlerEvent::OutDevice(d) => app.out_device = Some(d),
            }
        }

        if let Some(e) = convert(event.clone(), app.window.size().width as f64, app.window.size().height as f64) {
            use conrod::input::Button::*;
            // Handle all the basic Raw events in the entire window
            if let conrod::event::Input::Press(button) = e {
                match button {
                    Keyboard(key) => {
                        use conrod::input::Key;
                        match key {
                            Key::Space => try!(dictionary_commands_producer.send(DictionaryHandlerEvent::Refresh)
                                               .map_err(|_| Error::String("Cannot send".to_string()))),
                            Key::P => try!(dictionary_commands_producer.send(DictionaryHandlerEvent::Play)
                                               .map_err(|_| Error::String("Cannot send".to_string()))),
                            _ => { }
                        }
                    },
                    _ => { }
                }
            }

            ui.handle_event(e);
        }

        event.update(|_| {
            let ui = &mut ui.set_widgets();
            widget::Canvas::new().color(color::DARK_CHARCOAL).set(ids.canvas, ui);

            // Add reconstruct button
            if widget::Button::new()
                .w_h(200., 50.)
                .middle()
                .label("Reconstruct")
                .set(ids.reconstruct_button, ui)
                .was_clicked() 
            {
                dictionary_commands_producer.send(DictionaryHandlerEvent::Refresh);
            }

            // Add play button
            if widget::Button::new()
                .w_h(200., 50.)
                .label("Play")
                .set(ids.play_button, ui)
                .was_clicked()
            {
                dictionary_commands_producer.send(DictionaryHandlerEvent::Play);
            }


            if widget::Button::new()
                .w_h(200., 50.)
                .label("Start DSP")
                .left_from(ids.reconstruct_button, 20.)
                .rgb(0., 1., 0.)
                .set(ids.launch_audio_button, ui)
                .was_clicked() 
            {
                audio_commands_producer.push(AudioHandlerEvent::Start);
            }

            if widget::Button::new()
                .w_h(200., 50.)
                .label("Stop DSP")
                .down_from(ids.launch_audio_button, 20.)
                .rgb(1., 0., 0.)
                .set(ids.stop_audio_button, ui)
                .was_clicked() 
            {
                audio_commands_producer.push(AudioHandlerEvent::Stop);
            }

            match app.devices {
                Some(ref devices) => {
                    let ds: Vec<&str> = devices.iter().map(|d| d.1.as_str()).collect();
                    for idx in widget::DropDownList::new(&ds[..], app.in_device)
                        .w_h(200., 30.)
                        .label("Input Device")
                        .up_from(ids.launch_audio_button, 20.)
                        .set(ids.in_devices_list, ui) 
                    {
                        audio_commands_producer.push(AudioHandlerEvent::Setting(DeviceSetting::SetInDevice(idx as u32)));
                        app.in_device = Some(idx);
                    }

                    for idx in widget::DropDownList::new(&ds[..], app.out_device)
                        .w_h(200., 30.)
                        .label("Output Device")
                        .right_from(ids.in_devices_list, 20.)
                        .set(ids.out_devices_list, ui) 
                    {
                        audio_commands_producer.push(AudioHandlerEvent::Setting(DeviceSetting::SetOutDevice(idx as u32)));
                        app.out_device = Some(idx);
                    }
                }
                None => { }
            }
            
            for edit in widget::TextBox::new(&app.threshold_text)
                .align_text_middle()
                .w_h(200., 50.)
                .down_from(ids.play_button, 20.)
                .set(ids.threshold_box, ui) 
            {
                match edit {
                    widget::text_box::Event::Update(new_text) => {
                        app.threshold_text = new_text;
                    }
                    widget::text_box::Event::Enter => {
                        if let Ok(new_threshold) = app.threshold_text.parse::<usize>() {
                            dictionary_commands_producer.send(DictionaryHandlerEvent::SetThreshold(new_threshold));
                        }
                    }
                }
            }

            for edit in widget::TextBox::new(&app.depth_text) 
                .align_text_middle()
                .set(ids.depth_box, ui)
            {
                match edit {
                    widget::text_box::Event::Update(new_text) => {
                        app.depth_text = new_text;
                    }
                    widget::text_box::Event::Enter => {
                        if let Ok(new_depth) = app.depth_text.parse::<usize>() {
                            dictionary_commands_producer.send(DictionaryHandlerEvent::SetDepth(new_depth));
                        }
                    }
                }
            }
        });

        app.window.draw_2d(&event, |c, g| {
            if let Some(primitives) = ui.draw_if_changed() {
                let cache_queued_glyphs = |graphics: &mut G2d,
                                           cache: &mut G2dTexture,
                                           rect: conrod::text::rt::Rect<u32>,
                                           data: &[u8]|
                {
                    let offset = [rect.min.x, rect.min.y];
                    let size = [rect.width(), rect.height()];
                    let format = piston_window::texture::Format::Rgba8;
                    let encoder = &mut graphics.encoder;
                    text_vertex_data.clear();
                    text_vertex_data.extend(data.iter().flat_map(|&b| vec![255, 255, 255, b]));
                    UpdateTexture::update(cache, encoder, format, &text_vertex_data[..], offset, size)
                        .expect("failed to update texture")
                };
                    
                fn texture_from_image<T>(img: &T) -> &T { img };
                conrod::backend::piston::draw::primitives(primitives,
                                                          c, g,
                                                          &mut text_texture_cache,
                                                          &mut glyph_cache,
                                                          &image_map,
                                                          cache_queued_glyphs,
                                                          texture_from_image);
            }
        });
    }

    audio_commands_producer.push(AudioHandlerEvent::Quit);
    dictionary_commands_producer.send(DictionaryHandlerEvent::Quit);
    Ok(())
}

pub fn audio_handler<T>(audio_playback_queue: Arc<SegQueue<f64>>, audio_commands_receiver: Consumer<AudioHandlerEvent>, dict_prod: mpsc::Sender<DictionaryHandlerEvent>, gui_prod: mpsc::Sender<GuiHandlerEvent>) -> Result<(), Error<T>> {
    use AudioHandlerEvent::*;
    use DeviceSetting::*;

    let pa = try!(PortAudio::new());
    let mut settings: DuplexStreamSettings<f32, f32> = 
        try!(pa.default_duplex_stream_settings(1, 1, 44100., BLOCK_SIZE as u32)
             .map_err(Error::PortAudio));

    let devices = try!(pa.devices()).map(|d| {
        let d = d.unwrap();
        (d.0, d.1.name.to_string())
    }).collect();

    gui_prod.send(GuiHandlerEvent::InDevice(try!(pa.default_input_device()).0 as usize));
    gui_prod.send(GuiHandlerEvent::OutDevice(try!(pa.default_output_device()).0 as usize));
    gui_prod.send(GuiHandlerEvent::Devices(devices));
    let mut stream: Option<portaudio::Stream<portaudio::NonBlocking, portaudio::Duplex<_, _>>> = None;

    'audio: loop { 
        // Make sure the stream has not had an error
        stream.as_ref().map(|s| s.is_active());
        match audio_commands_receiver.try_pop() {
            Some(Setting(setting)) => {
                match setting {
                    SetInDevice(idx) => {
                        let info = try!(pa.device_info(DeviceIndex(idx)));
                        println!("Setting input device to {}", info.name);
                        settings.in_params = StreamParameters::new(DeviceIndex(idx), 1, true, info.default_low_input_latency);
                    },
                    SetOutDevice(idx) => {
                        let info = try!(pa.device_info(DeviceIndex(idx)));
                        println!("Setting output device to {}", info.name);
                        settings.out_params = StreamParameters::new(DeviceIndex(idx), 1, true, info.default_low_output_latency);
                    },
                }
            }
            Some(Start) => {
                println!("starting stream with {:?}", &settings);

                match stream {
                    Some(ref mut s) => try!(s.start()),
                    None => {
                        // Take another reference to the Arc containing the playback stream
                        let apq = audio_playback_queue.clone();
                        // Initialize the command queues
                        let (input_buffer_producer, input_buffer_receiver) = bounded_spsc_queue::make::<[f32; BLOCK_SIZE]>(65536);

                        let callback = move |DuplexStreamCallbackArgs { in_buffer, out_buffer, .. }| {

                            unsafe {
                                assert_eq!(BLOCK_SIZE, in_buffer.len());
                                let in_buffer: &[f32; BLOCK_SIZE] = transmute(in_buffer.as_ptr());
                                match input_buffer_producer.try_push(*in_buffer) {
                                    Some(_) => { println!("warning: sound buffer is full"); }
                                    None => { }
                                }
                            }

                            for s in out_buffer.iter_mut() {
                                match apq.try_pop() {
                                    Some(input) => { *s = input as f32 }
                                    None => { *s = 0. }
                                }
                            }

                            Continue
                        };

                        // settings is copy, so sending it is totally okay in this instance
                        stream = Some(try!(pa.open_non_blocking_stream(settings, callback)));
                        // Push the new stream receiver to the dictionary
                        dict_prod.send(DictionaryHandlerEvent::InputBuffer(Some(input_buffer_receiver)));
                        try!(stream.as_mut().unwrap().start());
                    },
                }
            }
            Some(Stop) => {
                match stream {
                    Some(ref mut s) => {
                        println!("stopping stream");
                        try!(s.stop());
                        dict_prod.send(DictionaryHandlerEvent::InputBuffer(None));
                    },
                    None => println!("Stream not enabled"),
                }
            }
            Some(Quit) => { 
                match stream {
                    Some(ref mut s) => try!(s.stop()),
                    None => println!("Stream not enabled"),
                }
                break 'audio;
            }
            None => { 
                thread::sleep(time::Duration::from_millis(10));
            }
        }
    }

    Ok(())
}

pub fn dictionary_handler(audio_playback_queue: Arc<SegQueue<f64>>, dictionary_commands_receiver: mpsc::Receiver<DictionaryHandlerEvent>) {
    // Read in the target file and create sequence using timestamps
    let assets = find_folder::Search::KidsThenParents(3, 5).for_folder("assets").unwrap();

    let target_sequence = {
        use std::borrow::Cow;

        let target = Arc::new(Sound::from_path(&assets.join("inventing.wav")).unwrap());
        println!("Source is {} samples", target.samples().len());

        // Only need mutable access for the training
        let partitioner = {
            let mut partitioner = Partitioner::new(Cow::Borrowed(&target));
            partitioner = partitioner.threshold(DEFAULT_THRESHOLD).depth(DEFAULT_DEPTH);
            partitioner.train();
            partitioner
        };

        let rows = target.mfccs().len() / NCOEFFS;
        let cols = NCOEFFS;
        let data = Matrix::new(rows, cols, target.mfccs().clone());
        let predictions = partitioner.predict(&data).unwrap();
        let splits = partitioner.partition(predictions).unwrap();

        println!("Found {} splits in original sound", splits.len());
        let dict = SoundDictionary::from_segments(&target, &splits[..]);
        let sequence = SoundSequence::new(dict.sounds);
        Arc::new(sequence)
    };

    use DictionaryHandlerEvent::*;

    let mut sound = Sound::from_samples(Vec::<f64>::with_capacity(65536), 44100., None, None);
    let mut buf = Vec::<f64>::with_capacity(65536);
    let mut depth = DEFAULT_DEPTH;
    let mut threshold = DEFAULT_THRESHOLD;
    let mut other_sound = Sound::from_samples(Vec::<f64>::new(), 44100., None, None);

    let target = target_sequence.to_sound();
    let mut partitioner = Partitioner::new(Cow::Owned(target))
        .threshold(threshold).depth(depth);
    partitioner.train();

    let mut input_buffer_receiver: Option<Consumer<[f32; BLOCK_SIZE]>> = None;

    loop {
        while let Some(Some(ref incoming_sound)) = input_buffer_receiver.as_mut().map(|r| r.try_pop()) {
            for s in incoming_sound.iter() {
                buf.push(*s as f64);
            }
            sound.push_samples(&buf[..]);
            buf.clear();
        };

        match dictionary_commands_receiver.try_recv() {
            Ok(Refresh) => {
                let rows = sound.mfccs().len() / NCOEFFS;
                let cols = NCOEFFS;
                let data = Matrix::new(rows, cols, sound.mfccs().clone());
                let predictions = partitioner.predict(&data).unwrap();
                let splits = partitioner.partition(predictions).unwrap();
                if splits.len() == 0 { 
                    println!("no possible partitions found");
                } else {
                    let dict = SoundDictionary::from_segments(&sound, &splits[..]);
                    println!("nsegs: {}", dict.sounds.len());
                    other_sound = target_sequence.clone_from_dictionary(&dict).unwrap().to_sound();
                    println!("samps: {}", other_sound.samples().len());
                }
            }
            Ok(Play) => {
                for s in other_sound.samples() {
                    audio_playback_queue.push(*s);
                }
            }
            Ok(SetThreshold(x)) => { 
                threshold = x; 
                partitioner = partitioner.threshold(threshold);
            }
            Ok(SetDepth(x)) => { 
                depth = x; 
                partitioner = partitioner.depth(depth);
            }
            Ok(InputBuffer(buf)) => {
                input_buffer_receiver = buf;
            }
            Ok(Quit) => { return; }
            Err(_) => { }
        }
    };
}

