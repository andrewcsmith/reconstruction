extern crate rusty_machine;
extern crate soundsym;
extern crate portaudio;
extern crate piston_window;
extern crate bounded_spsc_queue;
extern crate conrod;
extern crate find_folder;

use soundsym::*;
use portaudio::{Continue, DuplexStreamCallbackArgs, DuplexStreamSettings, PortAudio};
use bounded_spsc_queue::{Producer, Consumer};
use crossbeam::sync::SegQueue;
use rusty_machine::prelude::*;

use std::borrow::Cow;
use std::sync::Arc;
use std::mem::transmute;

use super::*;

const BLOCK_SIZE: usize = 64;

widget_ids! {
    pub struct Ids { canvas, plot, reconstruct_button, play_button, threshold_box, depth_box }
}

pub fn gui_handler(audio_commands_producer: Producer<AudioHandlerEvent>, dictionary_commands_producer: Producer<DictionaryHandlerEvent>) -> Result<(), Error> {
    use piston_window::{EventLoop, PistonWindow, UpdateEvent, WindowSettings, AdvancedWindow};
    const WIDTH: u32 = 400;
    const HEIGHT: u32 = 200;
    let mut window: PistonWindow = 
        try!(WindowSettings::new("Control window", [WIDTH, HEIGHT])
            .opengl(piston_window::OpenGL::V3_2)
            .samples(4)
            .decorated(false)
            .exit_on_esc(true)
            .build());
    window.set_position([0, 0]);
    window.set_ups(60);
    let mut ui = conrod::UiBuilder::new().build();
    let ids = Ids::new(ui.widget_id_generator());
    let assets = find_folder::Search::KidsThenParents(3, 5).for_folder("assets").unwrap();
    let font_path = assets.join("LH-Line1-Sans-Thin.ttf");
    try!(ui.fonts.insert_from_file(font_path));
    let mut text_texture_cache = conrod::backend::piston_window::GlyphCache::new(&mut window, WIDTH, HEIGHT);
    let image_map = conrod::image::Map::new();

    let mut threshold_text = String::new();
    let mut depth_text = String::new();

    while let Some(event) = window.next() {
        use input::{Event, Input, Button};
        use input::keyboard::Key;

        if let Some(e) = conrod::backend::piston_window::convert_event(event.clone(), &window) {
            ui.handle_event(e);
        }

        // Handle the raw events, primarily keyboard events
        match event {
            Event::Input(Input::Press(Button::Keyboard(key))) => { 
                match key {
                    Key::Space => {
                        dictionary_commands_producer.push(DictionaryHandlerEvent::Refresh);
                    }
                    Key::P => {
                        dictionary_commands_producer.push(DictionaryHandlerEvent::Play);
                    }
                    _ => { }
                }
            }
            _ => { }
        }

        event.update(|_| {
            use conrod::{color, widget, Colorable, Positionable, Sizeable, Widget, Labelable};
            let ui = &mut ui.set_widgets();
            widget::Canvas::new().color(color::DARK_CHARCOAL).set(ids.canvas, ui);

            let reconstruct_button = widget::Button::new()
                .w_h(200., 50.)
                .middle()
                .label("Reconstruct")
                .set(ids.reconstruct_button, ui);

            let play_button = widget::Button::new()
                .w_h(200., 20.)
                .label("Play")
                .set(ids.play_button, ui);
            
            for edit in widget::TextBox::new(&threshold_text)
                .align_text_middle()
                .set(ids.threshold_box, ui) 
            {
                match edit {
                    widget::text_box::Event::Update(new_text) => {
                        threshold_text = new_text;
                    }
                    widget::text_box::Event::Enter => {
                        if let Ok(new_threshold) = threshold_text.parse::<usize>() {
                            dictionary_commands_producer.push(DictionaryHandlerEvent::SetThreshold(new_threshold));
                        }
                    }
                }
            }

            for edit in widget::TextBox::new(&depth_text) 
                .align_text_middle()
                .set(ids.depth_box, ui)
            {
                match edit {
                    widget::text_box::Event::Update(new_text) => {
                        depth_text = new_text;
                    }
                    widget::text_box::Event::Enter => {
                        if let Ok(new_depth) = depth_text.parse::<usize>() {
                            dictionary_commands_producer.push(DictionaryHandlerEvent::SetDepth(new_depth));
                        }
                    }
                }
            }

            if reconstruct_button.was_clicked() {
                dictionary_commands_producer.push(DictionaryHandlerEvent::Refresh);
            }

            if play_button.was_clicked() {
                dictionary_commands_producer.push(DictionaryHandlerEvent::Play);
            }

        });

        window.draw_2d(&event, |c, g| {
            if let Some(primitives) = ui.draw_if_changed() {
                fn texture_from_image<T>(img: &T) -> &T { img };
                conrod::backend::piston_window::draw(c, g, primitives,
                                                     &mut text_texture_cache,
                                                     &image_map,
                                                     texture_from_image);
            }
        });
    }

    audio_commands_producer.push(AudioHandlerEvent::Quit);
    dictionary_commands_producer.push(DictionaryHandlerEvent::Quit);
    Ok(())
}

pub fn audio_handler(input_buffer_producer: Producer<[f32; BLOCK_SIZE]>, audio_playback_queue: Arc<SegQueue<f64>>, audio_commands_receiver: Consumer<AudioHandlerEvent>) -> Result<(), Error> {
    use AudioHandlerEvent::*;

    let pa = try!(PortAudio::new());
    let mut frames_elapsed: usize = 0;
    let settings: DuplexStreamSettings<f32, f32> = 
        try!(pa.default_duplex_stream_settings(1, 1, 44100., BLOCK_SIZE as u32).map_err(Error::PortAudio));
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
            match audio_playback_queue.try_pop() {
                Some(input) => { *s = input as f32 }
                None => { *s = 0. }
            }
        }

        frames_elapsed += 1;
        Continue
    };

    let mut stream = try!(pa.open_non_blocking_stream(settings, callback));
    println!("starting stream");
    try!(stream.start());

    while stream.is_active().unwrap_or(false) { 
        match audio_commands_receiver.try_pop() {
            Some(Quit) => { 
                try!(stream.stop()); 
            }
            None => { }
        }
    }

    Ok(())
}

pub fn dictionary_handler(input_buffer_receiver: Consumer<[f32; BLOCK_SIZE]>, target_sequence: Arc<SoundSequence>, audio_playback_queue: Arc<SegQueue<f64>>, dictionary_commands_receiver: Consumer<DictionaryHandlerEvent>) {
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

    loop {
        while let Some(incoming_sound) = input_buffer_receiver.try_pop() {
            for s in incoming_sound.iter() {
                buf.push(*s as f64);
            }
            sound.push_samples(&buf[..]);
            buf.clear();
        };

        match dictionary_commands_receiver.try_pop() {
            Some(Refresh) => {
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
            Some(Play) => {
                for s in other_sound.samples() {
                    audio_playback_queue.push(*s);
                }
            }
            Some(SetThreshold(x)) => { 
                threshold = x; 
                partitioner = partitioner.threshold(threshold);
            }
            Some(SetDepth(x)) => { 
                depth = x; 
                partitioner = partitioner.depth(depth);
            }
            Some(Quit) => { return; }
            _ => { }
        }
    };
}

