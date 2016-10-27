extern crate soundsym;
extern crate vox_box;
extern crate portaudio;
extern crate hound;
extern crate crossbeam;
extern crate bounded_spsc_queue;
extern crate piston_window;
extern crate find_folder;
extern crate input;

#[macro_use] extern crate conrod;

use soundsym::*;
use crossbeam::sync::SegQueue;

use std::sync::Arc;

mod error;
pub use error::Error;

mod events;
pub use events::*;

mod handlers;
pub use handlers::*;

const BLOCK_SIZE: usize = 64;
pub const DEFAULT_THRESHOLD: usize = 6;
pub const DEFAULT_DEPTH: usize = 5;

fn main() {
    run().unwrap();
}

fn run() -> Result<(), Error> {
    crossbeam::scope(|scope| {
        // Read in the target file and create sequence using timestamps
        let assets = find_folder::Search::KidsThenParents(3, 5).for_folder("assets").unwrap();

        let target_sequence = {
            use std::borrow::Cow;

            let target = Arc::new(Sound::from_path(&assets.join("inventing.wav")).unwrap());
            println!("Source is {} samples", target.samples().len());
            let partitioner = Partitioner::new(Cow::Borrowed(&target));
            let splits = partitioner.threshold(DEFAULT_THRESHOLD).depth(DEFAULT_DEPTH).partition().unwrap();
            println!("Found {} splits in original sound", splits.len());
            let dict = SoundDictionary::from_segments(&target, &splits[..]);
            let sequence = SoundSequence::new(dict.sounds);
            Arc::new(sequence)
        };

        // Initialize the command queues
        let (input_buffer_producer, input_buffer_receiver) = bounded_spsc_queue::make::<[f32; BLOCK_SIZE]>(65536);
        let (dictionary_commands_producer, dictionary_commands_receiver) = bounded_spsc_queue::make::<DictionaryHandlerEvent>(256);
        let (audio_commands_producer, audio_commands_receiver) = bounded_spsc_queue::make::<AudioHandlerEvent>(256);

        let audio_playback_queue = Arc::new(SegQueue::<f64>::new());
        let apq1 = audio_playback_queue.clone();
        let apq2 = audio_playback_queue.clone();
        scope.spawn(move || dictionary_handler(input_buffer_receiver, target_sequence, apq1, dictionary_commands_receiver));
        scope.spawn(move || audio_handler(input_buffer_producer, apq2, audio_commands_receiver));
        
        match gui_handler(audio_commands_producer, dictionary_commands_producer) {
            Err(e) => { 
                println!("abort! {}", e);
            }
            _ => { }
        }
    });

    Ok(())
}

