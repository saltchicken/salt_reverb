use nih_plug::prelude::*;
use fundsp::hacker32::*;
use std::sync::Arc;

struct SaltReverb {
    params: Arc<SaltReverbParams>,
    
    // The compiled fundsp audio graph. We box it dynamically so we 
    // don't have to write out its massive generated type signature.
    graph: Box<dyn AudioUnit>,
    
    // Shared variables allow us to pipe DAW automation into the fundsp graph
    // smoothly without allocating memory on the audio thread.
    cutoff_var: Shared,
}

#[derive(Params)]
struct SaltReverbParams {
    #[id = "mix"]
    pub mix: FloatParam,

    #[id = "cutoff"]
    pub cutoff: FloatParam,
}

impl Default for SaltReverbParams {
    fn default() -> Self {
        Self {
            mix: FloatParam::new(
                "Mix",
                0.5,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit(" %")
            .with_value_to_string(formatters::v2s_f32_percentage(2)),

            cutoff: FloatParam::new(
                "Damping Cutoff",
                2000.0,
                // A skewed range makes the knob feel natural when sweeping frequencies
                FloatRange::Skewed { min: 200.0, max: 20000.0, factor: FloatRange::skew_factor(-2.0) },
            )
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_hz_then_khz(1)),
        }
    }
}

impl Default for SaltReverb {
    fn default() -> Self {
        // 1. Create our shared parameter so the DAW can talk to fundsp
        let cutoff_var = shared(2000.0);
        
        // 2. Build the Left Lowpass Filter
        // `pass() | var()` takes the incoming audio, and stacks the cutoff frequency
        // next to it. `>> butterpass()` pipes that (Audio, Freq) bundle into the filter.
        let lpf_l = (pass() | var(&cutoff_var)) >> butterpass();
        
        // 3. Build the Right Lowpass Filter
        let lpf_r = (pass() | var(&cutoff_var)) >> butterpass();

        // 4. Combine them into a stereo filter using `|` (Stack)
        let stereo_lpf = lpf_l | lpf_r;

        // 5. Build the final Wet signal path. 
        // We use a massive 30-meter room size, 5-second decay, and 1.0 high-frequency damping,
        // then pipe `>>` the echoes into our stereo lowpass filter.
        let wet_graph = reverb_stereo(30.0, 5.0, 1.0) >> stereo_lpf;

        Self {
            params: Arc::new(SaltReverbParams::default()),
            graph: Box::new(wet_graph),
            cutoff_var,
        }
    }
}

impl Plugin for SaltReverb {
    const NAME: &'static str = "Salt Reverb";
    const VENDOR: &'static str = "Your Name";
    const URL: &'static str = env!("CARGO_PKG_HOMEPAGE");
    const EMAIL: &'static str = "your@email.com";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        aux_input_ports: &[],
        aux_output_ports: &[],
        names: PortNames::const_default(),
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        // Fundsp needs to know the DAW's sample rate to calculate delays/filters correctly
        self.graph.reset();
        self.graph.set_sample_rate(buffer_config.sample_rate as f64);
        true
    }

    fn reset(&mut self) {
        self.graph.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        
        // Iterate through the DAW audio buffer sample by sample
        for mut frame in buffer.iter_samples() {
            
            // Get the current smoothed knob values
            let mix = self.params.mix.smoothed.next();
            let cutoff = self.params.cutoff.smoothed.next();
            
            // Send the cutoff knob value into the fundsp graph
            self.cutoff_var.set_value(cutoff);

            // Turn the frame into an iterator and grab the left/right channels safely
            let mut channels = frame.into_iter();
            if let (Some(left), Some(right)) = (channels.next(), channels.next()) {
                
                let dry_l = *left;
                let dry_r = *right;

                // Tick the fundsp graph forward by exactly one sample
                let (wet_l, wet_r) = self.graph.filter_stereo(dry_l, dry_r);

                // Perform an equal-power Dry/Wet Crossfade
                *left = dry_l * (1.0 - mix) + wet_l * mix;
                *right = dry_r * (1.0 - mix) + wet_r * mix;
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for SaltReverb {
    const CLAP_ID: &'static str = "com.your-domain.salt_reverb";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Lush Atmospheric Reverb");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Reverb, ClapFeature::Stereo];
}

impl Vst3Plugin for SaltReverb {
    // VST3 strictly requires exactly 16 characters for the ID
    const VST3_CLASS_ID: [u8; 16] = *b"SaltReverbCloudV";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Reverb];
}

nih_export_clap!(SaltReverb);
nih_export_vst3!(SaltReverb);
