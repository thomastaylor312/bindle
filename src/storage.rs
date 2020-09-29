use anyhow::anyhow;
use sha2::{Digest, Sha256};
use std::fs::{create_dir_all, remove_dir_all, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The folder name for the invoices directory
const INVOICE_DIRECTORY: &str = "invoices";
/// The folder name for the boxes directory
const BOX_DIRECTORY: &str = "boxes";

pub trait Storage {
    /// This takes an invoice and creates it in storage.
    /// It must verify that each referenced box is present in storage. Any box that
    /// is not present must be returned in the list of IDs.
    fn create_invoice(&self, inv: &super::Invoice) -> Result<Vec<String>, anyhow::Error>;
    fn get_invoice(&self);
    // Remove an invoice
    //
    // Because invoices are not necessarily stored using just one field on the invoice,
    // the entire invoice must be passed to the deletion command.
    fn delete_invoice(&self, inv: &super::Invoice) -> Result<(), std::io::Error>;
    fn create_box(
        &self,
        label: &super::Label,
        data: std::io::BufReader<std::fs::File>,
    ) -> Result<(), anyhow::Error>;
    fn get_box();
    fn cleanup();
}

pub struct FileStorage {
    root: String, // TODO: this should be a path
}

impl FileStorage {
    /// Create a standard name for an invoice
    ///
    /// This is designed to create a repeatable opaque name when given an invoice.
    fn canonical_invoice_name(&self, inv: &crate::Invoice) -> String {
        // For now, hash name and version. Probably we should serialize the invoice and
        // generate the hash from that.
        let mut hasher = Sha256::new();
        hasher.update(inv.bindle.name.as_bytes());
        hasher.update(inv.bindle.version.as_bytes());
        let result = hasher.finalize();
        format!("{:x}", result)
    }

    /// Return the path to the invoice.toml file for the given invoice ID
    fn invoice_path(&self, invoice_id: &str) -> PathBuf {
        Path::new(self.root.as_str())
            .join(INVOICE_DIRECTORY)
            .join(invoice_id)
    }
    fn invoice_toml_path(&self, invoice_id: &str) -> PathBuf {
        self.invoice_path(invoice_id).join("invoice.toml")
    }
    /// Return the path to the box.toml file for the given box ID
    fn box_path(&self, box_id: &str) -> PathBuf {
        Path::new(self.root.as_str())
            .join(BOX_DIRECTORY)
            .join(box_id)
    }
    fn box_toml_path(&self, box_id: &str) -> PathBuf {
        self.box_path(box_id).join("box.toml")
    }
    /// Return the path to the box.dat file for the given box ID
    fn box_data_path(&self, box_id: &str) -> PathBuf {
        self.box_path(box_id).join("box.dat")
    }
}

impl Storage for FileStorage {
    fn create_invoice(&self, inv: &super::Invoice) -> Result<Vec<String>, anyhow::Error> {
        let invoice_cname = self.canonical_invoice_name(inv);
        let invoice_id = invoice_cname.as_str();

        // Create the base path if necessary
        let inv_path = self.invoice_path(invoice_id);
        if !inv_path.is_dir() {
            // If it exists and is a regular file, we have a problem
            if inv_path.is_file() {
                return Err(anyhow!(
                    "path already exists: {}",
                    inv_path.to_string_lossy()
                ));
            }
            create_dir_all(inv_path)?;
        }

        // Open the destination or error out if it already exists.
        let dest = self.invoice_toml_path(invoice_id);
        let mut out = OpenOptions::new()
            .create_new(true)
            .write(true)
            .read(true)
            .open(dest)?;

        // Encode the invoice into a TOML object
        let data = toml::to_vec(inv)?;
        out.write_all(data.as_slice())?;

        // TODO: Hash the contents of the file to make sure they match a given SHA.

        // Loop through the boxes and see what exists
        let mut missing = Vec::new();

        inv.boxes.iter().for_each(|k| {
            let boxpath = self.box_path(k.0);
            // Stat k to see if it exists. If it does not exist, add it.
            match std::fs::metadata(boxpath) {
                Ok(stat) => {
                    if !stat.is_dir() {
                        missing.push(k.0.to_owned())
                    }
                }
                Err(_e) => missing.push(k.0.to_owned()),
            }
        });

        Ok(missing)
    }
    fn get_invoice(&self) {}
    fn delete_invoice(&self, invoice: &super::Invoice) -> Result<(), std::io::Error> {
        remove_dir_all(self.invoice_path(self.canonical_invoice_name(invoice).as_str()))
    }
    fn create_box(
        &self,
        label: &super::Label,
        data: std::io::BufReader<std::fs::File>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
    fn get_box() {}
    fn cleanup() {}
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::Invoice;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    #[test]
    fn test_should_generate_paths() {
        let f = FileStorage {
            root: "test".to_owned(),
        };
        assert_eq!("test/invoices/123", f.invoice_path("123").to_str().unwrap());
        assert_eq!(
            "test/invoices/123/invoice.toml",
            f.invoice_toml_path("123").to_str().unwrap()
        );
        assert_eq!(
            "test/boxes/123".to_owned(),
            f.box_path("123").to_string_lossy()
        );
        assert_eq!(
            "test/boxes/123/box.toml".to_owned(),
            f.box_toml_path("123").to_string_lossy()
        );
        assert_eq!(
            "test/boxes/123/box.dat".to_owned(),
            f.box_data_path("123").to_string_lossy()
        );
    }

    #[test]
    fn test_should_create_delete_invoice() {
        // Create a temporary directory
        let root = tempdir().unwrap();
        let inv = invoice_fixture();
        let store = FileStorage {
            root: root.path().to_str().unwrap().to_owned(),
        };
        let inv_cname = store.canonical_invoice_name(&inv);
        let inv_name = inv_cname.as_str();
        // Create an file
        let missing = store.create_invoice(&inv).unwrap();
        assert_eq!(3, missing.len());

        // Out-of-band read the invoice
        assert!(store.invoice_toml_path(inv_name).exists());

        // Delete the invoice
        store.delete_invoice(&inv).unwrap();

        // Make sure the invoice is gone
        assert!(!store.invoice_path(inv_name).exists());

        // Drop the temporary directory
        assert!(root.close().is_ok());
    }

    fn invoice_fixture() -> Invoice {
        let mut boxen = BTreeMap::new();

        {
            let label = crate::Label {
                sha256: "abcdef1234567890987654321".to_owned(),
                media_type: "text/toml".to_owned(),
                name: "foo.toml".to_owned(),
                size: Some(101),
            };
            boxen.insert(label.sha256.to_string(), label);
        }
        {
            let label = crate::Label {
                sha256: "bbcdef1234567890987654321".to_owned(),
                media_type: "text/toml".to_owned(),
                name: "foo2.toml".to_owned(),
                size: Some(101),
            };
            boxen.insert(label.sha256.to_string(), label);
        }
        {
            let label = crate::Label {
                sha256: "cbcdef1234567890987654321".to_owned(),
                media_type: "text/toml".to_owned(),
                name: "foo3.toml".to_owned(),
                size: Some(101),
            };
            boxen.insert(label.sha256.to_string(), label);
        }

        Invoice {
            bindle: crate::BindleSpec {
                name: "foo".to_owned(),
                description: Some("bar".to_owned()),
                version: "v1.2.3".to_owned(),
                authors: Some(vec!["m butcher".to_owned()]),
                sha256: "abcdef1234567890987654321".to_owned(),
            },
            boxes: boxen,
        }
    }
}
