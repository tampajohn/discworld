#![allow(dead_code)]

use std::cmp;
use std::collections::HashMap;
use std::sync::Arc;

use crate::array_math::ArrayMath;
use crate::channel::Channel;
use crate::chorus::Chorus;
use crate::error::SynthesizerError;
use crate::region_pair::RegionPair;
use crate::reverb::Reverb;
use crate::soundfont::SoundFont;
use crate::soundfont_math::SoundFontMath;
use crate::synthesizer_settings::SynthesizerSettings;
use crate::voice_collection::VoiceCollection;

/// An instance of the SoundFont synthesizer.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Synthesizer {
    pub(crate) sound_font: Arc<SoundFont>,
    pub(crate) sample_rate: i32,
    pub(crate) block_size: usize,
    pub(crate) maximum_polyphony: usize,

    preset_lookup: HashMap<i32, usize>,
    default_preset: usize,

    channels: Vec<Channel>,

    voices: VoiceCollection,

    block_left: Vec<f32>,
    block_right: Vec<f32>,

    inverse_block_size: f32,

    block_read: usize,

    master_volume: f32,

    effects: Option<Effects>,
}

/// Mutable synthesis state; the immutable SoundFont remains external.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct SynthesizerCheckpoint {
    sample_rate: i32,
    block_size: usize,
    maximum_polyphony: usize,
    channels: Vec<Channel>,
    voices: VoiceCollection,
    block_left: Vec<f32>,
    block_right: Vec<f32>,
    inverse_block_size: f32,
    block_read: usize,
    master_volume: f32,
    effects: Option<Effects>,
}

impl Synthesizer {
    /// Capture all mutable DSP state at a render boundary.
    pub fn checkpoint(&self) -> SynthesizerCheckpoint {
        SynthesizerCheckpoint {
            sample_rate: self.sample_rate,
            block_size: self.block_size,
            maximum_polyphony: self.maximum_polyphony,
            channels: self.channels.clone(),
            voices: self.voices.clone(),
            block_left: self.block_left.clone(),
            block_right: self.block_right.clone(),
            inverse_block_size: self.inverse_block_size,
            block_read: self.block_read,
            master_volume: self.master_volume,
            effects: self.effects.clone(),
        }
    }

    /// Validate and install mutable state. The host must bind the checkpoint
    /// to the same SoundFont identity before calling this method.
    pub fn restore_checkpoint(&mut self, state: SynthesizerCheckpoint) -> Result<(), &'static str> {
        if !state.validate_checkpoint(&self.checkpoint(), self.sound_font.wave_data.len()) {
            return Err("invalid synthesizer checkpoint");
        }
        self.install_checkpoint(state);
        Ok(())
    }

    fn install_checkpoint(&mut self, state: SynthesizerCheckpoint) {
        self.sample_rate = state.sample_rate;
        self.block_size = state.block_size;
        self.maximum_polyphony = state.maximum_polyphony;
        self.channels = state.channels;
        self.voices = state.voices;
        self.block_left = state.block_left;
        self.block_right = state.block_right;
        self.inverse_block_size = state.inverse_block_size;
        self.block_read = state.block_read;
        self.master_volume = state.master_volume;
        self.effects = state.effects;
    }

    /// The number of channels.
    pub const CHANNEL_COUNT: usize = 16;
    /// The percussion channel.
    pub const PERCUSSION_CHANNEL: usize = 9;

    /// Initializes a new synthesizer using a specified SoundFont and settings.
    ///
    /// # Arguments
    ///
    /// * `sound_font` - The SoundFont instance.
    /// * `settings` - The settings for synthesis.
    pub fn new(
        sound_font: &Arc<SoundFont>,
        settings: &SynthesizerSettings,
    ) -> Result<Self, SynthesizerError> {
        settings.validate()?;

        let mut preset_lookup: HashMap<i32, usize> = HashMap::new();

        let mut min_preset_id = i32::MAX;
        let mut default_preset: usize = 0;
        for i in 0..sound_font.presets.len() {
            let preset = &sound_font.presets[i];

            // The preset ID is Int32, where the upper 16 bits represent the bank number
            // and the lower 16 bits represent the patch number.
            // This ID is used to search for presets by the combination of bank number
            // and patch number.
            let preset_id = (preset.bank_number << 16) | preset.patch_number;
            preset_lookup.insert(preset_id, i);

            // The preset with the minimum ID number will be default.
            // If the SoundFont is GM compatible, the piano will be chosen.
            if preset_id < min_preset_id {
                default_preset = i;
                min_preset_id = preset_id;
            }
        }

        let mut channels: Vec<Channel> = Vec::new();
        for i in 0..Synthesizer::CHANNEL_COUNT {
            channels.push(Channel::new(i == Synthesizer::PERCUSSION_CHANNEL));
        }

        let voices = VoiceCollection::new(settings);

        let block_left: Vec<f32> = vec![0_f32; settings.block_size];
        let block_right: Vec<f32> = vec![0_f32; settings.block_size];

        let inverse_block_size = 1_f32 / settings.block_size as f32;

        let block_read = settings.block_size;

        let master_volume = 0.5_f32;

        let effects = if settings.enable_reverb_and_chorus {
            Some(Effects::new(settings))
        } else {
            None
        };

        Ok(Self {
            sound_font: Arc::clone(sound_font),
            sample_rate: settings.sample_rate,
            block_size: settings.block_size,
            maximum_polyphony: settings.maximum_polyphony,
            preset_lookup,
            default_preset,
            channels,
            voices,
            block_left,
            block_right,
            inverse_block_size,
            block_read,
            master_volume,
            effects,
        })
    }

    /// Processes a MIDI message.
    ///
    /// # Arguments
    ///
    /// * `channel` - The channel to which the message will be sent.
    /// * `command` - The type of the message.
    /// * `data1` - The first data part of the message.
    /// * `data2` - The second data part of the message.
    pub fn process_midi_message(&mut self, channel: i32, command: i32, data1: i32, data2: i32) {
        if !(0 <= channel && channel < self.channels.len() as i32) {
            return;
        }

        let channel_info = &mut self.channels[channel as usize];

        match command {
            0x80 => self.note_off(channel, data1),       // Note Off
            0x90 => self.note_on(channel, data1, data2), // Note On
            0xB0 => match data1 // Controller
            {
                0x00 => channel_info.set_bank(data2), // Bank Selection
                0x01 => channel_info.set_modulation_coarse(data2), // Modulation Coarse
                0x21 => channel_info.set_modulation_fine(data2), // Modulation Fine
                0x06 => channel_info.data_entry_coarse(data2), // Data Entry Coarse
                0x26 => channel_info.data_entry_fine(data2), // Data Entry Fine
                0x07 => channel_info.set_volume_coarse(data2), // Channel Volume Coarse
                0x27 => channel_info.set_volume_fine(data2), // Channel Volume Fine
                0x0A => channel_info.set_pan_coarse(data2), // Pan Coarse
                0x2A => channel_info.set_pan_fine(data2), // Pan Fine
                0x0B => channel_info.set_expression_coarse(data2), // Expression Coarse
                0x2B => channel_info.set_expression_fine(data2), // Expression Fine
                0x40 => channel_info.set_hold_pedal(data2), // Hold Pedal
                0x5B => channel_info.set_reverb_send(data2), // Reverb Send
                0x5D => channel_info.set_chorus_send(data2), // Chorus Send
                0x63 => channel_info.set_nrpn_coarse(data2), // NRPN Coarse
                0x62 => channel_info.set_nrpn_fine(data2), // NRPN Fine
                0x65 => channel_info.set_rpn_coarse(data2), // RPN Coarse
                0x64 => channel_info.set_rpn_fine(data2), // RPN Fine
                0x78 => self.note_off_all_channel(channel, true), // All Sound Off
                0x79 => self.reset_all_controllers_channel(channel), // Reset All Controllers
                0x7B => self.note_off_all_channel(channel, false), // All Note Off
                _ => (),
            },
            0xC0 => channel_info.set_patch(data1), // Program Change
            0xE0 => channel_info.set_pitch_bend(data1, data2), // Pitch Bend
            _ => (),
        }
    }

    /// Stops a note.
    ///
    /// # Arguments
    ///
    /// * `channel` - The channel of the note.
    /// * `key` - The key of the note.
    pub fn note_off(&mut self, channel: i32, key: i32) {
        if !(0 <= channel && channel < self.channels.len() as i32) {
            return;
        }

        for voice in self.voices.get_active_voices().iter_mut() {
            if voice.channel() == channel && voice.key() == key {
                voice.end();
            }
        }
    }

    /// Starts a note.
    ///
    /// # Arguments
    ///
    /// * `channel` - The channel of the note.
    /// * `key` - The key of the note.
    /// * `velocity` - The velocity of the note.
    pub fn note_on(&mut self, channel: i32, key: i32, velocity: i32) {
        if velocity == 0 {
            self.note_off(channel, key);
            return;
        }

        if !(0 <= channel && channel < self.channels.len() as i32) {
            return;
        }

        let channel_info = &self.channels[channel as usize];

        let preset_id = (channel_info.get_bank_number() << 16) | channel_info.get_patch_number();

        let mut preset = self.default_preset;
        match self.preset_lookup.get(&preset_id) {
            Some(value) => preset = *value,
            None => {
                // Try fallback to the GM sound set.
                // Normally, the given patch number + the bank number 0 will work.
                // For drums (bank number >= 128), it seems to be better to select the standard set (128:0).
                let gm_preset_id = if channel_info.get_bank_number() < 128 {
                    channel_info.get_patch_number()
                } else {
                    128 << 16
                };

                // If no corresponding preset was found. Use the default one...
                if let Some(value) = self.preset_lookup.get(&gm_preset_id) {
                    preset = *value
                }
            }
        }

        let preset = &self.sound_font.presets[preset];
        for preset_region in preset.regions.iter() {
            if preset_region.contains(key, velocity) {
                let instrument = &self.sound_font.instruments[preset_region.instrument];
                for instrument_region in instrument.regions.iter() {
                    if instrument_region.contains(key, velocity) {
                        let region_pair = RegionPair::new(preset_region, instrument_region);

                        if let Some(value) = self.voices.request_new(instrument_region, channel) {
                            value.start(&region_pair, channel, key, velocity)
                        }
                    }
                }
            }
        }
    }

    /// Stops all the notes in the specified channel.
    ///
    /// # Arguments
    ///
    /// * `immediate` - If `true`, notes will stop immediately without the release sound.
    pub fn note_off_all(&mut self, immediate: bool) {
        if immediate {
            self.voices.clear();
        } else {
            for voice in self.voices.get_active_voices().iter_mut() {
                voice.end();
            }
        }
    }

    /// Stops all the notes in the specified channel.
    ///
    /// # Arguments
    ///
    /// * `channel` - The channel in which the notes will be stopped.
    /// * `immediate` - If `true`, notes will stop immediately without the release sound.
    pub fn note_off_all_channel(&mut self, channel: i32, immediate: bool) {
        if immediate {
            for voice in self.voices.get_active_voices().iter_mut() {
                if voice.channel() == channel {
                    voice.kill();
                }
            }
        } else {
            for voice in self.voices.get_active_voices().iter_mut() {
                if voice.channel() == channel {
                    voice.end();
                }
            }
        }
    }

    /// Resets all the controllers.
    pub fn reset_all_controllers(&mut self) {
        for channel in &mut self.channels {
            channel.reset_all_controllers();
        }
    }

    /// Resets all the controllers of the specified channel.
    ///
    /// # Arguments
    ///
    /// * `channel` - The channel to be reset.
    pub fn reset_all_controllers_channel(&mut self, channel: i32) {
        if !(0 <= channel && channel < self.channels.len() as i32) {
            return;
        }

        self.channels[channel as usize].reset_all_controllers();
    }

    /// Resets the synthesizer.
    pub fn reset(&mut self) {
        self.voices.clear();

        for channel in &mut self.channels {
            channel.reset();
        }

        if let Some(effects) = self.effects.as_mut() {
            effects.reverb.mute();
            effects.chorus.mute();
        }

        self.block_read = self.block_size;
    }

    /// Renders the waveform.
    ///
    /// # Arguments
    ///
    /// * `left` - The buffer of the left channel to store the rendered waveform.
    /// * `right` - The buffer of the right channel to store the rendered waveform.
    ///
    /// # Remarks
    ///
    /// The output buffers for the left and right must be the same length.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        if left.len() != right.len() {
            panic!("The output buffers for the left and right must be the same length.");
        }

        let left_length = left.len();

        let mut wrote = 0;
        while wrote < left_length {
            if self.block_read == self.block_size {
                self.render_block();
                self.block_read = 0;
            }

            let src_rem = self.block_size - self.block_read;
            let dst_rem = left_length - wrote;
            let rem = cmp::min(src_rem, dst_rem);

            for t in 0..rem {
                left[wrote + t] = self.block_left[self.block_read + t];
                right[wrote + t] = self.block_right[self.block_read + t];
            }

            self.block_read += rem;
            wrote += rem;
        }
    }

    fn render_block(&mut self) {
        self.voices
            .process(&self.sound_font.wave_data, &self.channels);

        self.block_left.fill(0_f32);
        self.block_right.fill(0_f32);
        for voice in self.voices.get_active_voices().iter_mut() {
            let previous_gain_left = self.master_volume * voice.previous_mix_gain_left;
            let current_gain_left = self.master_volume * voice.current_mix_gain_left;
            Synthesizer::write_block(
                previous_gain_left,
                current_gain_left,
                voice.block(),
                &mut self.block_left[..],
                self.inverse_block_size,
            );
            let previous_gain_right = self.master_volume * voice.previous_mix_gain_right;
            let current_gain_right = self.master_volume * voice.current_mix_gain_right;
            Synthesizer::write_block(
                previous_gain_right,
                current_gain_right,
                voice.block(),
                &mut self.block_right[..],
                self.inverse_block_size,
            );
        }

        if let Some(effects) = self.effects.as_mut() {
            let chorus = &mut effects.chorus;
            let chorus_input_left = &mut effects.chorus_input_left[..];
            let chorus_input_right = &mut effects.chorus_input_right[..];
            let chorus_output_left = &mut effects.chorus_output_left[..];
            let chorus_output_right = &mut effects.chorus_output_right[..];
            chorus_input_left.fill(0_f32);
            chorus_input_right.fill(0_f32);
            for voice in self.voices.get_active_voices().iter_mut() {
                let previous_gain_left = voice.previous_chorus_send * voice.previous_mix_gain_left;
                let current_gain_left = voice.current_chorus_send * voice.current_mix_gain_left;
                Synthesizer::write_block(
                    previous_gain_left,
                    current_gain_left,
                    voice.block(),
                    chorus_input_left,
                    self.inverse_block_size,
                );
                let previous_gain_right =
                    voice.previous_chorus_send * voice.previous_mix_gain_right;
                let current_gain_right = voice.current_chorus_send * voice.current_mix_gain_right;
                Synthesizer::write_block(
                    previous_gain_right,
                    current_gain_right,
                    voice.block(),
                    chorus_input_right,
                    self.inverse_block_size,
                );
            }
            chorus.process(
                chorus_input_left,
                chorus_input_right,
                chorus_output_left,
                chorus_output_right,
            );
            ArrayMath::multiply_add(
                self.master_volume,
                chorus_output_left,
                &mut self.block_left[..],
            );
            ArrayMath::multiply_add(
                self.master_volume,
                chorus_output_right,
                &mut self.block_right[..],
            );

            let reverb = &mut effects.reverb;
            let reverb_input = &mut effects.reverb_input[..];
            let reverb_output_left = &mut effects.reverb_output_left[..];
            let reverb_output_right = &mut effects.reverb_output_right[..];
            reverb_input.fill(0_f32);
            for voice in self.voices.get_active_voices().iter_mut() {
                let previous_gain = reverb.get_input_gain()
                    * voice.previous_reverb_send
                    * (voice.previous_mix_gain_left + voice.previous_mix_gain_right);
                let current_gain = reverb.get_input_gain()
                    * voice.current_reverb_send
                    * (voice.current_mix_gain_left + voice.current_mix_gain_right);
                Synthesizer::write_block(
                    previous_gain,
                    current_gain,
                    voice.block(),
                    &mut reverb_input[..],
                    self.inverse_block_size,
                );
            }

            reverb.process(reverb_input, reverb_output_left, reverb_output_right);
            ArrayMath::multiply_add(
                self.master_volume,
                reverb_output_left,
                &mut self.block_left[..],
            );
            ArrayMath::multiply_add(
                self.master_volume,
                reverb_output_right,
                &mut self.block_right[..],
            );
        }
    }

    fn write_block(
        previous_gain: f32,
        current_gain: f32,
        source: &[f32],
        destination: &mut [f32],
        inverse_block_size: f32,
    ) {
        if SoundFontMath::max(previous_gain, current_gain) < SoundFontMath::NON_AUDIBLE {
            return;
        }

        if (current_gain - previous_gain).abs() < 1.0E-3_f32 {
            ArrayMath::multiply_add(current_gain, source, destination);
        } else {
            let step = inverse_block_size * (current_gain - previous_gain);
            ArrayMath::multiply_add_slope(previous_gain, step, source, destination);
        }
    }

    /// Gets the SoundFont used as the audio source.
    pub fn get_sound_font(&self) -> &SoundFont {
        &self.sound_font
    }

    /// Gets the sample rate for synthesis.
    pub fn get_sample_rate(&self) -> i32 {
        self.sample_rate
    }

    /// Gets the block size for rendering waveform.
    pub fn get_block_size(&self) -> usize {
        self.block_size
    }

    /// Gets the number of maximum polyphony.
    pub fn get_maximum_polyphony(&self) -> usize {
        self.maximum_polyphony
    }

    /// Gets the value indicating whether reverb and chorus are enabled.
    pub fn get_enable_reverb_and_chorus(&self) -> bool {
        self.effects.is_some()
    }

    /// Gets the master volume.
    pub fn get_master_volume(&self) -> f32 {
        self.master_volume
    }

    /// Sets the master volume.
    ///
    /// # Arguments
    ///
    /// * `value` - The new value of the master volume.
    pub fn set_master_volume(&mut self, value: f32) {
        self.master_volume = value;
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct Effects {
    reverb: Reverb,
    reverb_input: Vec<f32>,
    reverb_output_left: Vec<f32>,
    reverb_output_right: Vec<f32>,

    chorus: Chorus,
    chorus_input_left: Vec<f32>,
    chorus_input_right: Vec<f32>,
    chorus_output_left: Vec<f32>,
    chorus_output_right: Vec<f32>,
}

impl Effects {
    fn new(settings: &SynthesizerSettings) -> Effects {
        Self {
            reverb: Reverb::new(settings.sample_rate),
            reverb_input: vec![0_f32; settings.block_size],
            reverb_output_left: vec![0_f32; settings.block_size],
            reverb_output_right: vec![0_f32; settings.block_size],
            chorus: Chorus::new(settings.sample_rate, 0.002, 0.0019, 0.4),
            chorus_input_left: vec![0_f32; settings.block_size],
            chorus_input_right: vec![0_f32; settings.block_size],
            chorus_output_left: vec![0_f32; settings.block_size],
            chorus_output_right: vec![0_f32; settings.block_size],
        }
    }
}

impl SynthesizerCheckpoint {
    pub(crate) fn validate_checkpoint(&self, template: &Self, wave_len: usize) -> bool {
        let _ = (template, wave_len);
        (self.sample_rate == template.sample_rate)
            && (self.block_size == template.block_size)
            && (self.maximum_polyphony == template.maximum_polyphony)
            && (self.channels.len() == template.channels.len())
            && (self
                .channels
                .iter()
                .zip(&template.channels)
                .all(|(a, b)| a.validate_checkpoint(b, wave_len)))
            && (self.voices.validate_checkpoint(&template.voices, wave_len))
            && (self.block_left.len() == template.block_left.len())
            && (self.block_left.iter().all(|v| v.is_finite()))
            && (self.block_right.len() == template.block_right.len())
            && (self.block_right.iter().all(|v| v.is_finite()))
            && (self.inverse_block_size.is_finite())
            && (self.master_volume.is_finite())
            && (match (&self.effects, &template.effects) {
                (Some(a), Some(b)) => a.validate_checkpoint(b, wave_len),
                (None, None) => true,
                _ => false,
            })
            && (self.block_read <= self.block_size)
            && (self.inverse_block_size.to_bits() == template.inverse_block_size.to_bits())
    }
}

impl Effects {
    pub(crate) fn validate_checkpoint(&self, template: &Self, wave_len: usize) -> bool {
        let _ = (template, wave_len);
        (self.reverb.validate_checkpoint(&template.reverb, wave_len))
            && (self.reverb_input.len() == template.reverb_input.len())
            && (self.reverb_input.iter().all(|v| v.is_finite()))
            && (self.reverb_output_left.len() == template.reverb_output_left.len())
            && (self.reverb_output_left.iter().all(|v| v.is_finite()))
            && (self.reverb_output_right.len() == template.reverb_output_right.len())
            && (self.reverb_output_right.iter().all(|v| v.is_finite()))
            && (self.chorus.validate_checkpoint(&template.chorus, wave_len))
            && (self.chorus_input_left.len() == template.chorus_input_left.len())
            && (self.chorus_input_left.iter().all(|v| v.is_finite()))
            && (self.chorus_input_right.len() == template.chorus_input_right.len())
            && (self.chorus_input_right.iter().all(|v| v.is_finite()))
            && (self.chorus_output_left.len() == template.chorus_output_left.len())
            && (self.chorus_output_left.iter().all(|v| v.is_finite()))
            && (self.chorus_output_right.len() == template.chorus_output_right.len())
            && (self.chorus_output_right.iter().all(|v| v.is_finite()))
    }
}
