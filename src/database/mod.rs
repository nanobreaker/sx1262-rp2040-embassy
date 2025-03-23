use ekv::flash::{self, PageID};
use embassy_rp::{
    flash::{Blocking, Flash},
    peripherals::FLASH,
};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};

use crate::{device::Device, error::Error, DatabaseResources};

const FLASH_SIZE: usize = 2 * 1024 * 1024;
extern "C" {
    static __config_start: u32;
}

// Workaround for alignment requirements.
#[repr(C, align(4))]
pub struct AlignedBuf<const N: usize>([u8; N]);

pub struct DbFlash<T: NorFlash + ReadNorFlash> {
    start: usize,
    flash: T,
}

pub type EkvDatabase = ekv::Database<DbFlash<Flash<'static, FLASH, Blocking, FLASH_SIZE>>, NoopRawMutex>;

#[derive(defmt::Format)]
pub enum DbKey {
    AppSKey,
    NewSKey,
    DevAddr,
}

impl From<&DbKey> for [u8; 1] {
    fn from(value: &DbKey) -> Self {
        match value {
            DbKey::AppSKey => [0x00],
            DbKey::NewSKey => [0x01],
            DbKey::DevAddr => [0x02],
        }
    }
}

pub trait Database {
    async fn put(&mut self, key: &DbKey, val: &[u8]) -> Result<(), Error>;
    async fn get(&mut self, key: &DbKey, buf: &mut [u8]) -> Option<usize>;
}

impl Database for EkvDatabase {
    async fn put(&mut self, key: &DbKey, value: &[u8]) -> Result<(), Error> {
        defmt::info!("Writing key {:?} value {=[u8]:#x} to flash", key, value);

        let mut wtx = self.write_transaction().await;
        let key: [u8; 1] = key.into();

        wtx.write(&key, value).await.expect("should write");
        wtx.commit().await.expect("should commit");

        defmt::info!("Commited data to flash");

        Ok(())
    }

    async fn get(&mut self, key: &DbKey, buf: &mut [u8]) -> Option<usize> {
        defmt::info!("Reading key {:?} from flash", key);

        let rtx = self.read_transaction().await;
        let key: [u8; 1] = key.into();

        rtx.read(&key, buf)
            .await
            .inspect(|_| defmt::info!("Successfully read data {=[u8]:#x} from flash", buf))
            .ok()
    }
}

impl Device<DatabaseResources> for EkvDatabase {
    type Info = ();

    async fn prepare(r: DatabaseResources) -> Result<Self, crate::error::Error> {
        let flash: DbFlash<Flash<_, _, FLASH_SIZE>> = DbFlash {
            flash: Flash::new_blocking(r.flash),
            start: unsafe { &__config_start as *const u32 as usize },
        };
        let db = ekv::Database::<_, NoopRawMutex>::new(flash, ekv::Config::default());

        Ok(db)
    }

    async fn init(&mut self) -> Result<Self::Info, crate::error::Error> {
        if self.mount().await.is_err() {
            defmt::info!("Formatting flash memory");
            self.format().await.unwrap();
        }

        Ok(())
    }
}

impl<T> flash::Flash for DbFlash<T>
where
    T: NorFlash + ReadNorFlash,
{
    type Error = T::Error;

    fn page_count(&self) -> usize {
        ekv::config::MAX_PAGE_COUNT
    }

    async fn erase(&mut self, page_id: PageID) -> Result<(), <DbFlash<T> as flash::Flash>::Error> {
        self.flash.erase(
            (self.start + page_id.index() * ekv::config::PAGE_SIZE) as u32,
            (self.start + page_id.index() * ekv::config::PAGE_SIZE + ekv::config::PAGE_SIZE) as u32,
        )
    }

    async fn read(&mut self, page_id: PageID, offset: usize, data: &mut [u8]) -> Result<(), <DbFlash<T> as flash::Flash>::Error> {
        let address = self.start + page_id.index() * ekv::config::PAGE_SIZE + offset;
        let mut buf = AlignedBuf([0; ekv::config::PAGE_SIZE]);
        self.flash.read(address as u32, &mut buf.0[..data.len()])?;
        data.copy_from_slice(&buf.0[..data.len()]);
        Ok(())
    }

    async fn write(&mut self, page_id: PageID, offset: usize, data: &[u8]) -> Result<(), <DbFlash<T> as flash::Flash>::Error> {
        let address = self.start + page_id.index() * ekv::config::PAGE_SIZE + offset;
        let mut buf = AlignedBuf([0; ekv::config::PAGE_SIZE]);
        buf.0[..data.len()].copy_from_slice(data);
        self.flash.write(address as u32, &buf.0[..data.len()])
    }
}
