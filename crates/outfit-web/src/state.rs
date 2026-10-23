use outfit_core::{CharacterPart, Garment, ProfileChoice, Sheet, Strategy};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum InputSource {
    Outfit {
        garment: Garment,
        profile: ProfileChoice,
        layers: Vec<(u64, Sheet)>,
    },
    Hat {
        image: u64,
    },
    Character {
        layers: Vec<(u64, CharacterPart)>,
    },
    Recolor {
        pairs: Vec<(u64, u64)>,
    },
}

/// Images are immutable. An upload identity plus its layout identifies a layer;
/// neither equality nor cache hits scan, hash, or duplicate any RGBA pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InputKey {
    pub source: InputSource,
    pub palette: bool,
    pub strategy: Strategy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reuse {
    New,
    Pending,
    Completed,
}

struct Entry<T> {
    key: InputKey,
    generation: u64,
    completed: bool,
    value: T,
}

/// Exactly one current entry, including its in-flight computation. A failed or
/// superseded computation cannot evict a newer entry when its await completes.
pub(crate) struct Latest<T> {
    entry: Option<Entry<T>>,
    generation: u64,
}

impl<T: Clone> Latest<T> {
    pub fn new() -> Self {
        Self {
            entry: None,
            generation: 0,
        }
    }

    pub fn clear(&mut self) {
        self.generation += 1;
        self.entry = None;
    }

    pub fn get(&self, key: &InputKey) -> Option<(T, Reuse, u64)> {
        self.entry
            .as_ref()
            .filter(|entry| entry.key == *key)
            .map(|entry| {
                (
                    entry.value.clone(),
                    if entry.completed {
                        Reuse::Completed
                    } else {
                        Reuse::Pending
                    },
                    entry.generation,
                )
            })
    }

    pub fn insert(&mut self, key: InputKey, value: T) -> (u64, Reuse) {
        self.generation += 1;
        self.entry = Some(Entry {
            key,
            generation: self.generation,
            completed: false,
            value,
        });
        (self.generation, Reuse::New)
    }

    pub fn finish(&mut self, generation: u64, success: bool) {
        if let Some(entry) = self
            .entry
            .as_mut()
            .filter(|entry| entry.generation == generation)
        {
            if success {
                entry.completed = true;
            } else {
                self.entry = None;
            }
        }
    }
}

/// Used independently by export/clipboard and by each upload slot. Starting a
/// replacement or clearing input retires every older completion in that lane.
#[derive(Default)]
pub(crate) struct Epoch(u64);

impl Epoch {
    pub fn advance(&mut self) -> u64 {
        self.0 += 1;
        self.0
    }

    pub fn accepts(&self, ticket: u64) -> bool {
        self.0 == ticket
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use outfit_core::Profile;
    use std::rc::Rc;

    fn key() -> InputKey {
        InputKey {
            source: InputSource::Outfit {
                garment: Garment::Chest,
                profile: ProfileChoice::Conservative,
                layers: vec![(7, Sheet::Body), (9, Sheet::FrontSleeves)],
            },
            palette: true,
            strategy: Strategy::Compressed,
        }
    }

    #[test]
    fn pending_and_completed_share_the_same_allocation() {
        let mut cache = Latest::new();
        let result = Rc::new(String::from("large encoding"));
        let (generation, first_reuse) = cache.insert(key(), Rc::clone(&result));
        assert_eq!(first_reuse, Reuse::New);
        let (pending, reuse, found) = cache.get(&key()).unwrap();
        assert_eq!(reuse, Reuse::Pending);
        assert_eq!(found, generation);
        assert!(Rc::ptr_eq(&result, &pending));
        cache.finish(generation, true);
        let (completed, reuse, _) = cache.get(&key()).unwrap();
        assert_eq!(reuse, Reuse::Completed);
        assert!(Rc::ptr_eq(&pending, &completed));
    }

    #[test]
    fn ordered_layer_identity_and_every_setting_are_part_of_the_key() {
        let mut cache = Latest::new();
        cache.insert(key(), ());
        for source in [
            InputSource::Outfit {
                garment: Garment::Chest,
                profile: ProfileChoice::Conservative,
                layers: vec![(9, Sheet::FrontSleeves), (7, Sheet::Body)],
            },
            InputSource::Outfit {
                garment: Garment::Chest,
                profile: ProfileChoice::Conservative,
                layers: vec![(8, Sheet::Body), (9, Sheet::FrontSleeves)],
            },
            InputSource::Outfit {
                garment: Garment::Chest,
                profile: ProfileChoice::Conservative,
                layers: vec![(7, Sheet::Back), (9, Sheet::FrontSleeves)],
            },
            InputSource::Outfit {
                garment: Garment::Chest,
                profile: ProfileChoice::Conservative,
                layers: vec![(7, Sheet::Body)],
            },
            InputSource::Outfit {
                garment: Garment::Pants,
                profile: ProfileChoice::Conservative,
                layers: vec![(7, Sheet::Body), (9, Sheet::FrontSleeves)],
            },
            InputSource::Outfit {
                garment: Garment::Chest,
                profile: ProfileChoice::Original,
                layers: vec![(7, Sheet::Body), (9, Sheet::FrontSleeves)],
            },
            InputSource::Outfit {
                garment: Garment::Chest,
                profile: ProfileChoice::Auto,
                layers: vec![(7, Sheet::Body), (9, Sheet::FrontSleeves)],
            },
            InputSource::Hat { image: 7 },
        ] {
            let mut changed = key();
            changed.source = source;
            assert!(cache.get(&changed).is_none());
        }
        let mut changed = key();
        changed.palette = false;
        assert!(cache.get(&changed).is_none());
        changed = key();
        changed.strategy = Strategy::Passes;
        assert!(cache.get(&changed).is_none());
    }

    #[test]
    fn hats_key_image_palette_and_strategy_separately_from_outfits() {
        let mut cache = Latest::new();
        let hat = InputKey {
            source: InputSource::Hat { image: 7 },
            palette: true,
            strategy: Strategy::Text,
        };
        cache.insert(hat.clone(), ());
        assert!(cache.get(&hat).is_some());
        assert!(cache.get(&key()).is_none());
        let mut changed = hat.clone();
        changed.source = InputSource::Hat { image: 8 };
        assert!(cache.get(&changed).is_none());
        changed = hat.clone();
        changed.palette = false;
        assert!(cache.get(&changed).is_none());
        for strategy in [Strategy::Compressed, Strategy::Passes] {
            changed = hat.clone();
            changed.strategy = strategy;
            assert!(cache.get(&changed).is_none());
        }
    }

    #[test]
    fn character_keys_keep_part_and_upload_identity_separate_from_equipment() {
        let mut cache = Latest::new();
        let character = InputKey {
            source: InputSource::Character {
                layers: vec![(7, CharacterPart::Body), (8, CharacterPart::Head)],
            },
            palette: true,
            strategy: Strategy::Text,
        };
        cache.insert(character.clone(), ());
        assert!(cache.get(&character).is_some());
        assert!(cache.get(&key()).is_none());
        for source in [
            InputSource::Hat { image: 7 },
            InputSource::Character {
                layers: vec![(7, CharacterPart::Head), (8, CharacterPart::Body)],
            },
            InputSource::Character {
                layers: vec![(7, CharacterPart::Body), (9, CharacterPart::Head)],
            },
            InputSource::Character {
                layers: vec![(7, CharacterPart::Body)],
            },
        ] {
            let mut changed = character.clone();
            changed.source = source;
            assert!(cache.get(&changed).is_none());
        }
        let mut changed = character.clone();
        changed.palette = false;
        assert!(cache.get(&changed).is_none());
        changed = character;
        changed.strategy = Strategy::Compressed;
        assert!(cache.get(&changed).is_none());
    }

    #[test]
    fn recolor_keys_include_both_uploads_and_ordered_pairs() {
        let mut cache = Latest::new();
        let recolor = InputKey {
            source: InputSource::Recolor {
                pairs: vec![(7, 8), (9, 10)],
            },
            palette: false,
            strategy: Strategy::Text,
        };
        cache.insert(recolor.clone(), ());
        assert!(cache.get(&recolor).is_some());
        assert!(cache.get(&key()).is_none());
        for pairs in [
            vec![(8, 7), (9, 10)],
            vec![(7, 11), (9, 10)],
            vec![(11, 8), (9, 10)],
            vec![(9, 10), (7, 8)],
            vec![(7, 8)],
            vec![],
        ] {
            let mut changed = recolor.clone();
            changed.source = InputSource::Recolor { pairs };
            assert!(cache.get(&changed).is_none());
        }
    }

    #[test]
    fn superseded_success_and_failure_cannot_change_the_current_entry() {
        let mut cache = Latest::new();
        let (first, _) = cache.insert(key(), 1);
        let (second, _) = cache.insert(key(), 2);
        cache.finish(first, false);
        assert_eq!(cache.get(&key()), Some((2, Reuse::Pending, second)));
        cache.finish(first, true);
        assert_eq!(cache.get(&key()), Some((2, Reuse::Pending, second)));
        cache.finish(second, false);
        assert!(cache.get(&key()).is_none());
        let (retry, _) = cache.insert(key(), 3);
        cache.finish(second, true);
        assert_eq!(cache.get(&key()), Some((3, Reuse::Pending, retry)));
        cache.finish(retry, true);
        assert_eq!(cache.get(&key()), Some((3, Reuse::Completed, retry)));
    }

    #[test]
    fn clear_releases_latest_input_and_retires_its_completion() {
        let mut cache = Latest::new();
        let result = Rc::new(String::from("encoding"));
        let (old, _) = cache.insert(key(), Rc::clone(&result));
        cache.clear();
        assert_eq!(Rc::strong_count(&result), 1);
        cache.finish(old, true);
        assert!(cache.get(&key()).is_none());
    }

    #[test]
    fn simultaneous_consumers_share_one_real_encoding_and_reuse_it_after_completion() {
        use futures::{FutureExt, channel::oneshot, executor::LocalPool, task::LocalSpawnExt};
        use std::cell::{Cell, RefCell};

        let mut executor = LocalPool::new();
        let spawner = executor.spawner();
        let runs = Rc::new(Cell::new(0));
        let results = Rc::new(RefCell::new(Vec::new()));
        let (release, ready) = oneshot::channel();
        let run_count = Rc::clone(&runs);
        let computation = async move {
            // A channel controls completion order, not encoder behavior. The
            // cached work below is the same real encoder used by exports.
            ready.await.unwrap();
            run_count.set(run_count.get() + 1);
            let mut rgba = vec![0; 86 * 258 * 4];
            rgba[(21 * 86 + 21) * 4..(21 * 86 + 21) * 4 + 4].copy_from_slice(&[17, 33, 65, 255]);
            let image = outfit_core::RgbaImage {
                width: 86,
                height: 258,
                rgba,
            };
            outfit_core::encode(
                &[outfit_core::Layer {
                    image: &image,
                    frames: outfit_core::layout(Sheet::Body),
                }],
                outfit_core::template(Garment::Chest, Profile::Conservative),
                true,
            )
            .map(Rc::new)
            .map_err(|error| error.to_string())
        }
        .boxed_local()
        .shared();
        let mut cache = Latest::new();
        let (generation, _) = cache.insert(key(), computation);
        for _ in 0..2 {
            let (future, reuse, _) = cache.get(&key()).unwrap();
            assert_eq!(reuse, Reuse::Pending);
            let results = Rc::clone(&results);
            spawner
                .spawn_local(async move {
                    let result = future.await.unwrap();
                    results.borrow_mut().push(result);
                })
                .unwrap();
        }
        executor.run_until_stalled();
        assert_eq!(runs.get(), 0);
        assert!(results.borrow().is_empty());
        release.send(()).unwrap();
        executor.run_until_stalled();
        assert_eq!(runs.get(), 1);
        assert_eq!(results.borrow().len(), 2);
        assert!(Rc::ptr_eq(&results.borrow()[0], &results.borrow()[1]));
        cache.finish(generation, true);
        let (future, reuse, _) = cache.get(&key()).unwrap();
        assert_eq!(reuse, Reuse::Completed);
        let completed = executor.run_until(future).unwrap();
        assert!(Rc::ptr_eq(&results.borrow()[0], &completed));
        assert_eq!(runs.get(), 1);
    }

    #[test]
    fn latest_request_wins_but_upload_slots_are_independent() {
        let mut output = Epoch::default();
        let old_clipboard = output.advance();
        let new_export = output.advance();
        assert!(!output.accepts(old_clipboard));
        assert!(output.accepts(new_export));
        output.advance(); // Setting change invalidates an export still compressing.
        assert!(!output.accepts(new_export));
        let mut body = Epoch::default();
        let mut sleeves = Epoch::default();
        let old_body = body.advance();
        let sleeve = sleeves.advance();
        let latest_body = body.advance();
        assert!(!body.accepts(old_body));
        assert!(body.accepts(latest_body));
        assert!(sleeves.accepts(sleeve));
        body.advance(); // Clearing a file cancels its pending read.
        assert!(!body.accepts(latest_body));
    }
}
