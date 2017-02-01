extern crate soundsym;
extern crate vox_box;
extern crate portaudio;
extern crate hound;
extern crate crossbeam;
extern crate bounded_spsc_queue;
extern crate find_folder;
extern crate rusty_machine;
extern crate piston_window;

#[macro_use] extern crate conrod;

use crossbeam::sync::SegQueue;

use std::sync::Arc;
use std::cell::RefCell;
use std::sync::mpsc;

mod error;
pub use error::Error;

mod events;
pub use events::*;

mod handlers;
pub use handlers::*;

fn main() {
    run::<DictionaryHandlerEvent>();
}

fn run<T>() -> Result<(), Error<T>> {
    crossbeam::scope(|scope| {
        let (audio_commands_producer, audio_commands_receiver) = bounded_spsc_queue::make::<AudioHandlerEvent>(256);

        let audio_playback_queue = Arc::new(SegQueue::<f64>::new());
        let (dict_prod, dict_cons) = mpsc::channel::<DictionaryHandlerEvent>();
        let (gui_prod, gui_recv) = mpsc::channel::<GuiHandlerEvent>();
        let audio_dict_prod = dict_prod.clone();
        let apq1 = audio_playback_queue.clone();
        let apq2 = audio_playback_queue.clone();
        scope.spawn(move || dictionary_handler(apq1, dict_cons));
        scope.spawn(move || audio_handler::<DictionaryHandlerEvent>(apq2, audio_commands_receiver, audio_dict_prod, gui_prod));
        match gui_handler::<DictionaryHandlerEvent>(audio_commands_producer, dict_prod, gui_recv) {
            Err(e) => { 
                println!("abort! {}", e);
            }
            Ok(_) => { }
        }
    });

    Ok(())
}

