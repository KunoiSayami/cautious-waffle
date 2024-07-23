mod v1 {
    use crate::cloudflare::ApiRequest;
    use crate::datastructures::Config;
    use log::{debug, error, info, warn};
    use notify::{Event, RecursiveMode, Watcher};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread::JoinHandle;
    use std::time::Duration;
    use tap::TapFallible;
    use tokio::sync::RwLock;

    #[derive(Debug)]
    struct DataToUpdate {
        path: String,
        data: Arc<RwLock<ApiRequest>>,
        relay_flag: Arc<AtomicBool>,
    }

    impl DataToUpdate {
        pub fn new(
            path: String,
            data: Arc<RwLock<ApiRequest>>,
            relay_flag: Arc<AtomicBool>,
        ) -> Self {
            Self {
                path,
                data,
                relay_flag,
            }
        }

        pub async fn update(&self) -> Option<()> {
            let config = Config::try_from_file(&self.path)
                .await
                .tap_err(|e| error!("[Can be safely ignored] Unable to parse new file: {e:?}"))
                .ok()?;

            let mut data = self.data.write().await;
            let relay = data.is_relay();
            let new_data = ApiRequest::try_from(config)
                .tap_err(|e| {
                    error!("[Can be safely ignored] Unable parse configure to inner type {e:?}")
                })
                .ok()?;
            if !relay && new_data.is_relay() {
                debug!("Server is running on relay mode");
            }
            *data = new_data;
            self.relay_flag.store(relay, Ordering::Relaxed);
            info!("Reload configure file successful, {}", data.info());
            Some(())
        }
    }

    #[derive(Debug)]
    pub struct FileWatchDog {
        handler: JoinHandle<Option<()>>,
        stop_signal_channel: oneshot::Sender<bool>,
    }

    impl FileWatchDog {
        pub fn file_watching(
            file: String,
            stop_signal_channel: oneshot::Receiver<bool>,
            data: Arc<RwLock<ApiRequest>>,
            relay_flag: Arc<AtomicBool>,
        ) -> Option<()> {
            let path = PathBuf::from(file.clone());

            let data = DataToUpdate::new(file, data, relay_flag);

            let mut watcher = notify::recommended_watcher(move |res| match res {
                Ok(event) => {
                    if Self::decide(event) {
                        tokio::runtime::Builder::new_current_thread()
                            .build()
                            .map(|runtime| runtime.block_on(data.update()))
                            .tap_err(|e| {
                                error!("[Can be safely ignored] Unable create runtime: {e:?}")
                            })
                            .ok();
                    }
                }
                Err(e) => {
                    error!("[Can be safely ignored] Got error while watching file {e:?}")
                }
            })
            .tap_err(|e| error!("[Can be safely ignored] Can't start watcher {e:?}"))
            .ok()?;

            watcher
                .watch(&path, RecursiveMode::NonRecursive)
                .tap_err(|e| error!("[Can be safely ignored] Unable to watch file: {e:?}"))
                .ok()?;

            stop_signal_channel
                .recv()
                .tap_err(|e| {
                    error!("[Can be safely ignored] Got error while poll oneshot event: {e:?}")
                })
                .ok();

            watcher
                .unwatch(&path)
                .tap_err(|e| error!("[Can be safely ignored] Unable to unwatch file: {e:?}"))
                .ok()?;

            debug!("File watcher exited!");
            Some(())
        }

        fn decide(event: Event) -> bool {
            if let notify::EventKind::Access(notify::event::AccessKind::Close(
                notify::event::AccessMode::Write,
            )) = event.kind
            {
                return true;
            }
            event.need_rescan()
        }

        pub fn start(
            path: String,
            data: Arc<RwLock<ApiRequest>>,
            relay_flag: Arc<AtomicBool>,
        ) -> Self {
            let (stop_signal_channel, receiver) = oneshot::channel();
            Self {
                handler: std::thread::spawn(|| {
                    Self::file_watching(path, receiver, data, relay_flag)
                }),
                stop_signal_channel,
            }
        }

        pub fn stop(self) -> Option<()> {
            if !self.handler.is_finished() {
                self.stop_signal_channel
                    .send(true)
                    .tap_err(|e| {
                        error!(
                "[Can be safely ignored] Unable send terminate signal to file watcher thread: {e:?}"
            )
                    })
                    .ok()?;
                std::thread::spawn(move || {
                    for _ in 0..5 {
                        std::thread::sleep(Duration::from_millis(100));
                        if self.handler.is_finished() {
                            break;
                        }
                    }
                    if !self.handler.is_finished() {
                        warn!("[Can be safely ignored] File watching not finished yet.");
                    }
                })
                .join()
                .unwrap();
            }
            Some(())
        }
    }
}

pub use v1::*;
