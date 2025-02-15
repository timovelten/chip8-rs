use rodio::{cpal::FromSample, OutputStream, Sample, Sink, Source};

pub struct Player {
    /// Rodio requires this to not be dropped. Fine...
    _stream: OutputStream,
    sink: rodio::Sink,
}

impl Player {
    pub fn new<S>(source: S) -> Self
    where
        S: Source + Send + 'static,
        f32: FromSample<S::Item>,
        S::Item: Sample + Send,
    {
        let (_stream, stream_handle) = OutputStream::try_default().unwrap();
        let sink = Sink::try_new(&stream_handle).unwrap();
        sink.pause();
        sink.set_volume(0.1);

        sink.append(source);

        Self { _stream, sink }
    }

    pub fn set(&self, b: bool) {
        match (b, self.sink.is_paused()) {
            (true, true) => self.sink.play(),
            (false, false) => self.sink.pause(),
            (true, false) | (false, true) => {}
        }
    }
}

/// An infinite source that produces a sine.
///
/// Always has a rate of 48kHz and one channel.
/// Very much build after 'SineWave' from rodio/sine.rs
#[derive(Clone, Debug)]
pub struct SquareWave {
    freq: f32,
    num_sample: usize,
}

impl SquareWave {
    /// The frequency of the sine.
    #[inline]
    pub fn new(freq: u32) -> Self {
        Self {
            freq: freq as f32,
            num_sample: 0,
        }
    }
}

impl Iterator for SquareWave {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        self.num_sample = self.num_sample.wrapping_add(1);

        let x = 2.0 * self.freq * self.num_sample as f32 / 48000.0;
        if x as u64 % 2 == 0 {
            Some(1.0)
        } else {
            Some(-1.0)
        }
    }
}

impl rodio::Source for SquareWave {
    #[inline]
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    #[inline]
    fn channels(&self) -> u16 {
        1
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
        48000
    }

    #[inline]
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}
