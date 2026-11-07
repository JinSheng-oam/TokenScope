use super::{validate_catalog, ModelPriceEntry};
use crate::error::{Result, StoreError};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_MANUAL_ENTRIES: usize = 1000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PricingPreferences {
    pub keyword_matching: bool,
    pub manual_entries: Vec<ModelPriceEntry>,
}

impl Default for PricingPreferences {
    fn default() -> Self {
        Self {
            keyword_matching: true,
            manual_entries: Vec::new(),
        }
    }
}

impl PricingPreferences {
    fn file(db: &Path) -> PathBuf {
        db.with_extension("pricing.json")
    }

    pub fn read(db: &Path) -> Result<Self> {
        let file = match fs::File::open(Self::file(db)) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(_) => return Err(StoreError::InvalidInput("pricing preferences file")),
        };
        let mut data = Vec::new();
        file.take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut data)
            .map_err(|_| StoreError::InvalidInput("pricing preferences file"))?;
        if data.len() as u64 > MAX_FILE_BYTES {
            return Err(StoreError::InvalidInput("pricing preferences size"));
        }
        let preferences: Self = serde_json::from_slice(&data)
            .map_err(|_| StoreError::InvalidInput("pricing preferences JSON"))?;
        preferences.validate()?;
        Ok(preferences)
    }

    fn validate(&self) -> Result<()> {
        if self.manual_entries.len() > MAX_MANUAL_ENTRIES {
            return Err(StoreError::InvalidInput("manual price count"));
        }
        if self
            .manual_entries
            .iter()
            .any(|entry| entry.input_rate_nanos.is_none() || entry.output_rate_nanos.is_none())
        {
            return Err(StoreError::InvalidInput("manual input or output rate"));
        }
        if self.manual_entries.is_empty() {
            return Ok(());
        }
        validate_catalog(
            &format!("models.dev:{}", "0".repeat(64)),
            &self.manual_entries,
            0,
            None,
        )
    }

    pub fn write(&self, db: &Path) -> Result<()> {
        self.validate()?;
        let data = serde_json::to_vec(self)
            .map_err(|_| StoreError::InvalidInput("pricing preferences JSON"))?;
        if data.len() as u64 > MAX_FILE_BYTES {
            return Err(StoreError::InvalidInput("pricing preferences size"));
        }
        let target = Self::file(db);
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| StoreError::InvalidInput("pricing preferences timestamp"))?
            .as_nanos();
        let temp = target.with_extension(format!("{}.{}.tmp", std::process::id(), unique));
        let result = (|| -> io::Result<()> {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp)?;
            file.write_all(&data)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp, &target)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result.map_err(|_| StoreError::InvalidInput("pricing preferences write"))
    }
}
