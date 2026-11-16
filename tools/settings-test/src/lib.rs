//! Host test for `shared/src/settings_codec.rs` (included by path: it has no hardware in it).

#[path = "../../../shared/src/settings_codec.rs"]
mod settings_codec;

#[cfg(test)]
mod tests {
    use super::settings_codec::{Settings, RECORD_LEN};
    use psx_display::Brightness;
    use psx_mc::{Card, RamCard};

    const FILE_NAME: &str = "BESLES-00000CELSTCC1";

    fn file_of(image: &[u8]) -> Vec<u8> {
        let mut card = Card::new(RamCard::from_image(image).expect("a 128 KiB card image"));
        let mut buf = [0u8; 128];
        let len = card
            .read(FILE_NAME, &mut buf)
            .expect("the settings file is on the card");
        buf[..len].to_vec()
    }

    /// A card the shipping release (celeste-collection-psx c273443) wrote when the SFX slider was
    /// lowered one step and the settings menu closed: the `CCS1` file as it really was.
    #[test]
    fn the_real_old_card_loads_with_its_settings() {
        let file = file_of(include_bytes!("../fixtures/ccs1-card-sfx7.mcd"));
        assert_eq!(&file[0..4], b"CCS1", "the fixture is an old-format file");
        let (settings, legacy) = Settings::decode(&file).expect("the old file decodes");
        assert!(legacy, "reported as the old format");
        assert_eq!(settings.sfx, 7);
        assert_eq!(settings.music, 8);
        assert_eq!(settings.pixel_scale, file[6]);
        assert_eq!(settings.side_preset, file[7]);
        assert_eq!(settings.brightness, Brightness::from_byte(file[8]));
    }

    #[test]
    fn the_file_before_brightness_loads_with_the_default() {
        let old = [b'C', b'C', b'S', b'1', 3, 5, 2, 1];
        let (settings, legacy) = Settings::decode(&old).unwrap();
        assert!(legacy);
        assert_eq!(
            settings,
            Settings {
                sfx: 3,
                music: 5,
                pixel_scale: 2,
                side_preset: 1,
                brightness: Brightness::DEFAULT
            }
        );
    }

    #[test]
    fn foreign_and_short_files_are_refused() {
        assert!(Settings::decode(b"CCS1").is_none());
        assert!(Settings::decode(b"NOPE12345").is_none());
        assert!(Settings::decode(&[]).is_none());
    }

    #[test]
    fn every_setting_round_trips_through_the_record() {
        for sfx in 0..=8u8 {
            for music in 0..=8u8 {
                for scale in 1..=2u8 {
                    for preset in 0..4u8 {
                        for level in -5..=5i8 {
                            let settings = Settings {
                                sfx,
                                music,
                                pixel_scale: scale,
                                side_preset: preset,
                                brightness: Brightness::new(level),
                            };
                            let bytes = settings.encode().unwrap();
                            assert_eq!(bytes.len(), RECORD_LEN);
                            let (back, legacy) = Settings::decode(&bytes).unwrap();
                            assert!(!legacy);
                            assert_eq!(back, settings);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_migrated_file_reads_back_as_the_same_settings() {
        let file = file_of(include_bytes!("../fixtures/ccs1-card-sfx7.mcd"));
        let (old, _) = Settings::decode(&file).unwrap();
        let rewritten = old.encode().unwrap();
        assert_eq!(Settings::decode(&rewritten).unwrap().0, old);
    }
}
