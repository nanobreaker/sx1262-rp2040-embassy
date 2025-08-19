use ekv::flash::{self, PageID};
use embassy_rp::flash::{Blocking, Flash};
use embassy_rp::peripherals::FLASH;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};

use crate::storage::{Key, Storage};
use crate::FlashRes;

const FLASH_SIZE: usize = 2 * 1024 * 1024;

extern "C" {
    static __config_start: u32;
}

#[repr(C, align(4))]
pub struct AlignedBuf<const N: usize>([u8; N]);
pub struct DbFlash<T: NorFlash + ReadNorFlash> {
    start: usize,
    flash: T,
}

#[derive(defmt::Format)]
pub enum FlashStorageError {
    Mount(ekv::MountError<embassy_rp::flash::Error>),
    Format(ekv::FormatError<embassy_rp::flash::Error>),
    Write(ekv::WriteError<embassy_rp::flash::Error>),
    Commit(ekv::CommitError<embassy_rp::flash::Error>),
}

pub struct FlashStorage {
    flash: ekv::Database<DbFlash<Flash<'static, FLASH, Blocking, FLASH_SIZE>>, NoopRawMutex>,
}

impl FlashStorage {
    pub fn new(r: FlashRes) -> Self {
        let flash = {
            let db_flash: DbFlash<Flash<_, _, FLASH_SIZE>> = DbFlash {
                flash: Flash::new_blocking(r.flash),
                start: unsafe { &__config_start as *const u32 as usize },
            };

            ekv::Database::<_, NoopRawMutex>::new(db_flash, ekv::Config::default())
        };

        Self { flash }
    }
}

impl Storage for FlashStorage {
    type Error = FlashStorageError;

    async fn put(&mut self, key: &Key, value: &[u8]) -> Result<(), Self::Error> {
        defmt::debug!("Writing key {:?} value {=[u8]:#x} to flash", key, value);

        let mut wtx = self.flash.write_transaction().await;
        let key: [u8; 1] = key.into();

        if let Err(e) = wtx.write(&key, value).await {
            return Err(FlashStorageError::Write(e));
        }

        if let Err(e) = wtx.commit().await {
            return Err(FlashStorageError::Commit(e));
        }

        Ok(())
    }

    async fn get(&mut self, key: &Key, buf: &mut [u8]) -> Option<usize> {
        let rtx = self.flash.read_transaction().await;
        let key: [u8; 1] = key.into();

        rtx.read(&key, buf)
            .await
            .inspect(|_| defmt::debug!("Read key {:?} value {=[u8]:#x}", key, buf))
            .ok()
    }

    async fn mount(&mut self) -> Result<(), Self::Error> {
        match self.flash.mount().await {
            Ok(()) => Ok(()),
            Err(e) => Err(FlashStorageError::Mount(e)),
        }
    }

    async fn format(&mut self) -> Result<(), Self::Error> {
        match self.flash.format().await {
            Ok(()) => Ok(()),
            Err(e) => Err(FlashStorageError::Format(e)),
        }
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
