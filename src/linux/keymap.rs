use std::collections::{HashMap, VecDeque};
use std::convert::TryInto;
use std::fmt::Display;

use log::{debug, trace};
pub(super) use xkeysym::{KeyCode, Keysym};

use crate::{Direction, InputError, InputResult, Key};

#[derive(Debug)]
pub(super) struct KeyMapMapping<Keycode> {
    pub(super) additionally_mapped: HashMap<Keysym, Keycode>,
    keycode_min: Keycode,
    keycode_max: Keycode,
    keysyms_per_keycode: u8,
    keysyms: Vec<u32>,
    unused_keycodes: VecDeque<Keycode>,
}

#[derive(Debug)]
struct KeyMapState<Keycode> {
    held_keycodes: Vec<Keycode>, // cannot get unmapped
    needs_regeneration: bool,
    #[cfg(feature = "x11rb")]
    last_keys: Vec<Keycode>, // last pressed keycodes
}

#[derive(Debug)]
pub struct KeyMap<Keycode> {
    pub(super) keymap_mapping: KeyMapMapping<Keycode>,
    keymap_state: KeyMapState<Keycode>,

    #[cfg(feature = "x11rb")]
    delay: u32, // milliseconds
    #[cfg(feature = "x11rb")]
    last_event_before_delays: std::time::Instant, // time of the last event
}

impl<Keycode> KeyMap<Keycode>
where
    Keycode: Copy + Clone + PartialEq + Display,
    Keycode: TryInto<usize> + TryFrom<usize>,
    <Keycode as TryInto<usize>>::Error: std::fmt::Debug,
    <Keycode as TryFrom<usize>>::Error: std::fmt::Debug,
{
    /// Create a new `KeyMap`
    pub fn new(
        keycode_min: Keycode,
        keycode_max: Keycode,
        unused_keycodes: VecDeque<Keycode>,
        keysyms_per_keycode: u8,
        keysyms: Vec<u32>,
        #[cfg(feature = "x11rb")] delay: u32,
    ) -> Self {
        let capacity: usize = keycode_max.try_into().unwrap() - keycode_min.try_into().unwrap();
        let capacity = capacity + 1;
        let keymap = HashMap::with_capacity(capacity);

        let keymap_state = KeyMapState {
            held_keycodes: vec![],
            needs_regeneration: true,
            #[cfg(feature = "x11rb")]
            last_keys: vec![],
        };

        let keymap_mapping = KeyMapMapping {
            additionally_mapped: keymap,
            keycode_min,
            keycode_max,
            keysyms_per_keycode,
            keysyms,
            unused_keycodes,
        };

        #[cfg(feature = "x11rb")]
        let last_event_before_delays = std::time::Instant::now();

        Self {
            keymap_mapping,
            keymap_state,
            #[cfg(feature = "x11rb")]
            delay,
            #[cfg(feature = "x11rb")]
            last_event_before_delays,
        }
    }

    fn keysym_to_keycode(&self, keysym: Keysym, level: u8) -> Option<Keycode> {
        let keycode_min: usize = self.keymap_mapping.keycode_min.try_into().unwrap();
        let keycode_max: usize = self.keymap_mapping.keycode_max.try_into().unwrap();
        for i in keycode_min..=keycode_max {
            let raw_code: u32 = i.try_into().unwrap();
            let min_code: u32 = keycode_min.try_into().unwrap();
            if xkeysym::keysym(
                KeyCode::from(raw_code),
                level,
                KeyCode::from(min_code),
                self.keymap_mapping.keysyms_per_keycode,
                &self.keymap_mapping.keysyms,
            ) == Some(keysym)
            {
                trace!("found keysym in row {i}, col {level}");
                return Some(i.try_into().unwrap());
            }
        }
        None
    }

    // Clicks can own Shift just for the text event. Held Unicode keys still
    // need layer-independent mappings so unrelated held keys are unaffected.
    #[cfg(feature = "x11rb")]
    pub fn shifted_keycode(&self, key: Key) -> Option<Keycode> {
        let sym = Keysym::from(key);
        if self.keysym_to_keycode(sym, 0).is_some() {
            None
        } else {
            self.keysym_to_keycode(sym, 1)
        }
    }

    // Try to enter the key
    #[allow(clippy::unnecessary_wraps)]
    pub fn key_to_keycode<C: Bind<Keycode>>(&mut self, c: &C, key: Key) -> InputResult<Keycode> {
        let sym = Keysym::from(key);

        if let Some(keycode) = self.keysym_to_keycode(sym, 0) {
            return Ok(keycode);
        }

        let keycode = {
            if let Some(&keycode) = self.keymap_mapping.additionally_mapped.get(&sym) {
                // The keysym is already mapped and cached in the keymap
                keycode
            } else {
                // Unmap keysyms if there are no unused keycodes
                self.make_room(c)?;
                // The keysym needs to get mapped to an unused keycode.
                // Always map the keycode if it has not yet been mapped, so it is layer agnostic
                self.map(c, sym)?
            }
        };

        Ok(keycode)
    }

    #[cfg(feature = "x11rb")]
    pub fn delay(&self) -> u32 {
        self.delay
    }

    #[cfg(feature = "x11rb")]
    pub fn set_delay(&mut self, delay: u32) {
        self.delay = delay;
    }

    /// Add the Keysym to the keymap
    ///
    /// This does not apply the changes
    pub fn map<C: Bind<Keycode>>(&mut self, c: &C, keysym: Keysym) -> InputResult<Keycode> {
        match self.keymap_mapping.unused_keycodes.pop_front() {
            // A keycode is unused so a mapping is possible
            Some(unused_keycode) => {
                trace!("trying to map keycode {unused_keycode} to keysym {keysym:?}");
                if c.bind_key(unused_keycode, keysym).is_err() {
                    return Err(InputError::Mapping(format!("{keysym:?}")));
                }
                self.keymap_state.needs_regeneration = true;
                self.keymap_mapping
                    .additionally_mapped
                    .insert(keysym, unused_keycode);
                debug!("mapped keycode {unused_keycode} to keysym {keysym:?}");
                Ok(unused_keycode)
            }
            // All keycodes are being used. A mapping is not possible
            None => Err(InputError::Mapping(format!("{keysym:?}"))),
        }
    }

    /// Remove the Keysym from the keymap
    ///
    /// This does not apply the changes
    pub fn unmap<C: Bind<Keycode>>(
        &mut self,
        c: &C,
        keysym: Keysym,
        keycode: Keycode,
    ) -> InputResult<()> {
        trace!("trying to unmap keysym {keysym:?}");
        if c.bind_key(keycode, Keysym::NoSymbol).is_err() {
            return Err(InputError::Unmapping(format!("{keysym:?}")));
        }
        self.keymap_state.needs_regeneration = true;
        self.keymap_mapping.unused_keycodes.push_back(keycode);
        self.keymap_mapping.additionally_mapped.remove(&keysym);
        debug!("unmapped keysym {keysym:?}");
        Ok(())
    }

    // Compute the delay for this event, including keys with existing mappings.
    #[cfg(feature = "x11rb")]
    pub fn delay_for(&mut self, keycode: Keycode, now: std::time::Instant) -> u32 {
        // Check if a delay is needed
        // A delay is required, if one of the keycodes was recently entered and there
        // was no delay between it

        // e.g. A quick rabbit
        // Chunk 1: 'A quick' # Add a delay before the second space
        // Chunk 2: ' rab'     # Add a delay before the second 'b'
        // Chunk 3: 'bit'     # Enter the remaining chars

        let delay = if self.keymap_state.last_keys.contains(&keycode) {
            let elapsed_ms = now
                .saturating_duration_since(self.last_event_before_delays)
                .as_millis()
                .try_into()
                .unwrap_or(u32::MAX);
            trace!("delay needed");
            self.keymap_state.last_keys.clear();
            self.delay.saturating_sub(elapsed_ms)
        } else {
            trace!("no repeated-key delay needed");
            self.delay.min(1)
        };
        self.keymap_state.last_keys.push(keycode);
        delay
    }

    /// Check if there are still unused keycodes available. If there aren't,
    /// make some room by freeing the already mapped keycodes.
    /// Returns true, if keys were unmapped and the keymap needs to be
    /// regenerated
    fn make_room<C: Bind<Keycode>>(&mut self, c: &C) -> InputResult<()> {
        // Unmap all keys, if all keycodes are already being used
        if self.keymap_mapping.unused_keycodes.is_empty() {
            let mapped_keys = self.keymap_mapping.additionally_mapped.clone();
            let held_keycodes = self.keymap_state.held_keycodes.clone();
            let mut made_room = false;

            for (&sym, &keycode) in mapped_keys
                .iter()
                .filter(|(_, keycode)| !held_keycodes.contains(keycode))
            {
                self.unmap(c, sym, keycode)?;
                made_room = true;
            }
            if made_room {
                return Ok(());
            }
            return Err(InputError::Unmapping("all keys that were mapped are also currently held. no way to make room for new mappings".to_string()));
        }
        Ok(())
    }

    pub fn key(&mut self, keycode: Keycode, direction: Direction) {
        match direction {
            Direction::Press => {
                debug!("added the key {keycode} to the held keycodes");
                self.keymap_state.held_keycodes.push(keycode);
            }
            Direction::Release => {
                debug!("removed the key {keycode} from the held keycodes");
                self.keymap_state.held_keycodes.retain(|&k| k != keycode);
            }
            Direction::Click => (),
        }

        #[cfg(feature = "x11rb")]
        {
            self.last_event_before_delays = std::time::Instant::now();
        }
    }
}

pub trait Bind<Keycode> {
    // Map the keysym to the given keycode
    // Only use keycodes that are not used, otherwise the existing mapping is
    // overwritten
    // If the keycode is mapped to the NoSymbol keysym, the key is unbound and can
    // get used again later
    fn bind_key(&self, _: Keycode, _: Keysym) -> Result<(), ()> {
        Ok(()) // No need to do anything
    }
}

impl<Keycode> Bind<Keycode> for () {}

#[cfg(all(test, feature = "x11rb"))]
mod delay_tests {
    use super::KeyMap;
    use std::{
        collections::VecDeque,
        time::{Duration, Instant},
    };

    fn keymap(delay: u32, now: Instant) -> KeyMap<u8> {
        let mut keymap = KeyMap::new(8, 9, VecDeque::new(), 1, vec![0, 0], delay);
        keymap.last_event_before_delays = now;
        keymap
    }

    #[test]
    fn zero_delay_applies_to_new_and_repeated_keys() {
        let now = Instant::now();
        let mut keymap = keymap(0, now);
        for keycode in [8, 9, 8, 8, 9] {
            assert_eq!(keymap.delay_for(keycode, now), 0);
        }
    }

    #[test]
    fn repeated_keys_use_configured_delay_minus_elapsed_time() {
        let now = Instant::now();
        let mut keymap = keymap(17, now);
        assert_eq!(keymap.delay_for(8, now), 1);
        assert_eq!(keymap.delay_for(8, now + Duration::from_millis(7)), 10);
        assert_eq!(keymap.delay_for(8, now + Duration::from_millis(18)), 0);
    }

    #[test]
    fn a_repeated_key_delay_is_not_reused_for_the_next_distinct_key() {
        let now = Instant::now();
        let mut keymap = keymap(17, now);
        assert_eq!(keymap.delay_for(8, now), 1);
        assert_eq!(keymap.delay_for(8, now), 17);
        assert_eq!(keymap.delay_for(9, now), 1);
    }

    #[test]
    fn constructor_and_setter_share_the_authoritative_delay() {
        let now = Instant::now();
        let mut constructed = keymap(25, now);
        let mut updated = keymap(12, now);
        updated.set_delay(25);
        assert_eq!(constructed.delay(), updated.delay());
        for keycode in [8, 9, 8, 8] {
            assert_eq!(
                constructed.delay_for(keycode, now),
                updated.delay_for(keycode, now)
            );
        }
        updated.set_delay(0);
        assert_eq!(updated.delay(), 0);
        assert_eq!(updated.delay_for(8, now), 0);
        updated.set_delay(9);
        assert_eq!(updated.delay_for(8, now), 9);
    }
}

#[cfg(all(test, feature = "x11rb"))]
mod shifted_mapping_tests {
    use super::{KeyMap, Keysym};
    use crate::Key;
    use std::collections::VecDeque;

    #[test]
    fn shifted_letters_and_symbols_use_existing_level_one() {
        let keymap = KeyMap::new(
            8_u8,
            9,
            VecDeque::new(),
            2,
            vec![
                Keysym::a.raw(),
                Keysym::A.raw(),
                Keysym::_1.raw(),
                Keysym::exclam.raw(),
            ],
            0,
        );
        assert_eq!(keymap.shifted_keycode(Key::Unicode('A')), Some(8));
        assert_eq!(keymap.shifted_keycode(Key::Unicode('!')), Some(9));
        assert_eq!(keymap.shifted_keycode(Key::Unicode('a')), None);
        assert_eq!(keymap.shifted_keycode(Key::Unicode('1')), None);
        assert_eq!(keymap.shifted_keycode(Key::Unicode('é')), None);
        assert!(keymap.keymap_mapping.additionally_mapped.is_empty());
    }
}
