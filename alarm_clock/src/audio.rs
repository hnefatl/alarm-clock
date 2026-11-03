use core_s3::audio::AudioSource;
use core_s3::audio::{Aw88298, SpeakerConfig};
use defmt::{debug, error};
use esp_hal::{
    Blocking, dma_tx_buffer,
    i2c::master::I2c,
    i2s::master::{Channels, DataFormat, I2s, TdmConfig},
    peripherals::{DMA_CH1, GPIO13, GPIO33, GPIO34, I2S1},
    time::Rate,
};

pub fn init(internal_i2c: I2c<'static, Blocking>) -> I2c<'static, Blocking> {
    let mut amp = Aw88298::new(internal_i2c);
    amp.init(SpeakerConfig::DEFAULT).expect("failed to init amplifier");
    amp.release()
}

pub struct AudioPeripherals<'a> {
    pub i2s1: I2S1<'a>,
    pub dma_ch1: DMA_CH1<'a>,
    pub gpio13: GPIO13<'a>,
    pub gpio33: GPIO33<'a>,
    pub gpio34: GPIO34<'a>,
}

pub async fn play_audio<'a, S: AudioSource>(peripherals: AudioPeripherals<'a>, mut source: S) {
    // Cribbed from
    // https://github.com/anapeksha/core-s3-rs/blob/3d7fc571faa2fd7b2f45535b5f8400702b6678b4/examples/sound_wav/src/main.rs#L111C25-L111C38

    // I2S is "inter-IC sound"
    let i2s = I2s::new(
        peripherals.i2s1,
        peripherals.dma_ch1,
        TdmConfig::new_tdm_philips()
            .with_sample_rate(Rate::from_hz(16_000))
            .with_data_format(DataFormat::Data16Channel16)
            .with_channels(Channels::MONO),
    )
    .expect("failed to init i2s");
    let i2s = i2s.into_async();

    // Transmit channel for the I2S.
    let mut i2s_tx = i2s
        .i2s_tx
        .with_bclk(peripherals.gpio34)
        .with_ws(peripherals.gpio33)
        .with_dout(peripherals.gpio13)
        .build();

    const BUFFER_LENGTH: usize = 4096;
    assert!(BUFFER_LENGTH % size_of::<i16>() == 0);

    let mut tx_buffer = dma_tx_buffer!(BUFFER_LENGTH).expect("failed to init tx buffer");
    let mut data = heapless::Vec::<u8, BUFFER_LENGTH>::new();
    debug!("Audio: starting playing...");
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
        tx_buffer.fill(&data);
        debug!("Audio: filled {} bytes", data.len());

        let transfer = match i2s_tx.write(tx_buffer) {
            Ok(transfer) => transfer,
            Err((e, _, _)) => {
                error!("Failed I2S write: {}", e);
                return;
            }
        };
        debug!("Audio: started transfer");
        (i2s_tx, tx_buffer) = match transfer.wait_async().await {
            (Ok(()), returned_i2s_tx, returned_tx_buffer) => (returned_i2s_tx, returned_tx_buffer),
            (Err(e), _, _) => {
                error!("Failed I2S write: {}", e);
                return;
            }
        };
        debug!("Audio: finished transfer");
    }
    debug!("Audio: finished playing.");
}
