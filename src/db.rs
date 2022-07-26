//! cheapo database-ish sort of file to store state

use serde::{ Deserialize, Serialize };
use std::fs as sync_fs;
use std::path;
use std::sync::{ Arc, Mutex, MutexGuard };
use std::thread::panicking;
use std::time::SystemTime;
use tokio::fs as async_fs;

#[derive(Clone)]
pub struct DatabaseThing {
	inner: Arc<Mutex<DatabaseThingInner>>
}

struct DatabaseThingInner {
	pub meta: DatabaseThingMeta,
	pub data: DatabaseThingData
}

struct DatabaseThingMeta {
	pub filename: String,
	pub last_write_call_time: SystemTime
}

#[derive(Clone, Deserialize, Serialize)]
struct DatabaseThingData {
	pub saved_on_panic: bool,
	pub packages: Vec<PackageState>
}

use package_state::*;
pub use package_state::{ PackageNew, Repository };
mod package_state {
	use super::*;
	#[derive(Clone, Deserialize, Serialize)]
	pub enum PackageState {
		New(PackageNew),
		IssueFiled(PackageIssueFiled)
	}

	#[derive(Clone, Deserialize, Serialize)]
	pub struct PackageNew {
		pub name: String,
		pub repository: Repository,
		pub downloads: u32,
		pub stargazers_count: u32
	}

	#[derive(Clone, Deserialize, Serialize)]
	pub struct Repository {
		pub r#type: String,
		pub url: String
	}

	#[derive(Clone, Deserialize, Serialize)]
	pub struct PackageIssueFiled {
		pub name: String,
		pub repository: (String, String),
		pub downloads: u32,
		pub stargazers_count: u32
	}
}

impl DatabaseThing {
	pub async fn new(filename: &str) -> crate::Result<Self> {
		let data = if path::Path::new(filename).exists() {
			let data = async_fs::read(filename).await
				.map_err(|e| format!("error reading file {filename}: {e}"))?;

			let data = String::from_utf8(data)
				.map_err(|e| format!("error parsing text in file {filename}: {e}"))?;

			ron::from_str(&data)
				.map_err(|e| format!("error parsing ron in file {filename}: {e}"))?
		} else {
			let data = DatabaseThingData {
				saved_on_panic: false,
				packages: Vec::new()
			};
			// let ser_data = ron::to_string(&data)?;
			let ser_data = ron::ser::to_string_pretty(&data, Self::pretty_config())?;
			async_fs::write(filename, ser_data).await?;
			data
		};

		let new = Self {
			inner: Arc::new(Mutex::new(DatabaseThingInner {
				meta: DatabaseThingMeta {
					filename: filename.into(),
					last_write_call_time: SystemTime::now()
				},
				data
			}))
		};

		Ok(new)
	}

	pub fn add_package(&self, package: &PackageNew) -> Result<(), String> {
		let mut inner = self.lock_inner();
		inner.data.packages.push(PackageState::New(package.clone()));

		Ok(())
	}

	pub fn contains_package(&self, package_name: &str) -> bool {
		let inner = self.lock_inner();

		for package in inner.data.packages.iter() {
			let res = match package {
				PackageState::New(PackageNew { name, .. }) => { name == package_name }
				PackageState::IssueFiled(PackageIssueFiled { name, .. }) => { name == package_name }
			};
			if res { return true }
		}

		false
	}

	fn write_to_file_immediately(&self) {
		fn write_to_file_immediately_inner(db: &DatabaseThing) -> crate::Result {
			let mut inner = db.lock_inner();

			let now = SystemTime::now();
			inner.meta.last_write_call_time = now;

			let data = ron::ser::to_string_pretty(&inner.data, DatabaseThing::pretty_config())?;
			let filename = inner.meta.filename.clone();
			drop(inner);

			sync_fs::write(&filename, &data)?;
			Ok(())
		}

		let res = write_to_file_immediately_inner(self);
		if let Err(e) = res {
			println!("error when writing database file: {e}");
		}
	}

	fn lock_inner(&self) -> MutexGuard<'_, DatabaseThingInner> {
		match self.inner.lock() {
			Ok(lock) => { lock }
			Err(e) => { e.into_inner() }
		}
	}

	fn pretty_config() -> ron::ser::PrettyConfig {
		ron::ser::PrettyConfig::new()
			.new_line("\n".into())
			.indentor("\t".into())
			.struct_names(true)
	}
}

impl Drop for DatabaseThing {
	fn drop(&mut self) {
		let mut inner = self.lock_inner();

		let panicking = panicking();
		inner.data.saved_on_panic = panicking;
		if panicking { println!("db dropped because of panick!") }

		println!(
			"db stats:\n   total packages: {}",
			inner.data.packages.len()
		);

		// without this we deadlock on the next call to `self.write_to_file_immediately();`
		drop(inner);

		self.write_to_file_immediately();
	}
}
