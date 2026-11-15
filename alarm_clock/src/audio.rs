use anyhow::{Context, anyhow};
use core::mem::MaybeUninit;
use core_s3::audio::AudioSource;
use core_s3::audio::{Aw88298, SpeakerConfig};
use defmt::debug;
use esp_hal::Async;
use esp_hal::dma::DmaTxBuf;
use esp_hal::i2s::master::I2sTx;
use esp_hal::{
    Blocking, dma_tx_buffer,
    i2c::master::I2c,
    i2s::master::{Channels, DataFormat, I2s, TdmConfig},
    peripherals::{DMA_CH1, GPIO13, GPIO33, GPIO34, I2S1},
    time::Rate,
};

const BUFFER_LENGTH: usize = 4096;

pub struct Audio<'a> {
    // This is maybeuninit to play some tricks and make the interface `&mut self`
    // instead of a harder-to-wield `self`
    inner: MaybeUninit<InnerI2S<'a>>,
}
impl<'a> Audio<'a> {
    pub fn new(
        i2s1: I2S1<'a>,
        dma_ch1: DMA_CH1<'a>,
        gpio13: GPIO13<'a>,
        gpio33: GPIO33<'a>,
        gpio34: GPIO34<'a>,
    ) -> anyhow::Result<Self> {
        assert!(BUFFER_LENGTH % size_of::<i16>() == 0);

        let tx_buffer = dma_tx_buffer!(BUFFER_LENGTH).context("failed to init tx buffer")?;

        // Cribbed from
        // https://github.com/anapeksha/core-s3-rs/blob/3d7fc571faa2fd7b2f45535b5f8400702b6678b4/examples/sound_wav/src/main.rs#L111C25-L111C38

        // I2S is "inter-IC sound"
        let i2s = I2s::new(
            i2s1,
            dma_ch1,
            TdmConfig::new_tdm_philips()
                .with_sample_rate(Rate::from_hz(16_000))
                .with_data_format(DataFormat::Data16Channel16)
                .with_channels(Channels::MONO),
        )
        .expect("failed to init i2s");
        let i2s = i2s.into_async();

        let i2s_tx = i2s.i2s_tx.with_bclk(gpio34).with_ws(gpio33).with_dout(gpio13).build();

        Ok(Self {
            inner: MaybeUninit::new(InnerI2S { i2s_tx, tx_buffer }),
        })
    }

    pub async fn play_audio<S: AudioSource>(&mut self, source: S) -> anyhow::Result<()> {
        // Convert the reference into a value, storing a temporary uninit value.
        let inner = unsafe { core::mem::replace(&mut self.inner, MaybeUninit::uninit()).assume_init() };

        // Play the audio getting the resources back by value.
        let (inner, result) = inner.play_audio(source).await;

        // Re-init the inner reference, dropping the uninit value.
        let _ = core::mem::replace(&mut self.inner, MaybeUninit::new(inner));
        result
    }
}

pub fn init(internal_i2c: I2c<'static, Blocking>) -> I2c<'static, Blocking> {
    let mut amp = Aw88298::new(internal_i2c);
    amp.init(SpeakerConfig::DEFAULT).expect("failed to init amplifier");
    amp.release()
}

/// A hard-to-work-with by-value owner of the i2s bus and resources.
struct InnerI2S<'a> {
    i2s_tx: I2sTx<'a, Async>,
    tx_buffer: DmaTxBuf,
}
impl<'a> InnerI2S<'a> {
    async fn play_audio<S: AudioSource>(mut self, mut source: S) -> (Self, anyhow::Result<()>) {
        debug!("Audio: starting playing...");
        let mut data = heapless::Vec::<u8, BUFFER_LENGTH>::new();
        loop {
            data.clear();
            while !data.is_full() {
                let Some(s) = source.next_sample() else { break };
                // This can panic if the vec is overfilled: but due to the const math above, that
                // should never happen: we should always exactly fill or underfill the vec.
                data.extend(s.to_le_bytes());
            }
            if data.is_empty() {
                break;
            };
            self.tx_buffer.fill(&data);
            debug!("Audio: filled {} bytes", data.len());

            let transfer = match self.i2s_tx.write(self.tx_buffer) {
                Ok(transfer) => transfer,
                Err((e, i2s_tx, tx_buffer)) => {
                    return (Self { i2s_tx, tx_buffer }, Err(anyhow!("Failed I2S write: {:?}", e)));
                }
            };
            debug!("Audio: started transfer");

            (self.i2s_tx, self.tx_buffer) = match transfer.wait_async().await {
                (Ok(()), i2s_tx, tx_buffer) => (i2s_tx, tx_buffer),
                (Err(e), i2s_tx, tx_buffer) => {
                    return (Self { i2s_tx, tx_buffer }, Err(anyhow!("Failed I2S wait: {:?}", e)));
                }
            };
            debug!("Audio: finished transfer");
        }
        debug!("Audio: finished playing.");
        (self, Ok(()))
    }
}
