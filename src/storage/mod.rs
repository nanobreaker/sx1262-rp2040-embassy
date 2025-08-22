pub mod flash_storage;

#[derive(defmt::Format)]
#[allow(clippy::enum_variant_names)]
pub enum Key {
    AppSKey,
    NewSKey,
    DevAddr,
}

impl From<&Key> for [u8; 1] {
    fn from(value: &Key) -> Self {
        match value {
            Key::AppSKey => [0x00],
            Key::NewSKey => [0x01],
            Key::DevAddr => [0x02],
        }
    }
}

/// Trait to represent all needed operatios with the key-value storage
pub trait Storage {
    /// Error type representation, left up to the implementor
    type Error;

    /// Mounting flash
    async fn mount(&mut self) -> Result<(), Self::Error>;

    /// Formating flash, useful on first init
    async fn format(&mut self) -> Result<(), Self::Error>;

    /// Put a value with associated key
    async fn put(&mut self, key: &Key, val: &[u8]) -> Result<(), Self::Error>;

    /// Get value by associated key
    async fn get(&mut self, key: &Key, buf: &mut [u8]) -> Option<usize>;
}
