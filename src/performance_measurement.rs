use crate::config::LoadProfile;
use tokio::{sync::watch::{self, Receiver}, task::JoinSet};

pub enum Commands {
    Init,
    Start(Vec<u32>),
    Stop(Vec<u32>),
}

pub struct Stats {
    requests_sent: u64,
    errors_num: u64,
    response_time_millis: Vec<u64>,
}

pub struct Worker {
    id: u32,
    commands_channel: Receiver<Commands>,
    api_endpoint: String,
    active: bool,
    // TODO: statistic struct in Mutex
}

impl Worker {
    pub fn new(id: u32, commands_channel: Receiver<Commands>, api_endpoint: String) -> Self {
        Self {
            id,
            commands_channel,
            api_endpoint,
            active: false,
        }
    }

    pub async fn run(&mut self) {
        let mut counter = 5;

        loop {
            if let Ok(has_changed) = self.commands_channel.has_changed() {
                if has_changed {
                    let msg = self.commands_channel.borrow_and_update();

                    match &(*msg) {
                        Commands::Init => {
                            println!("Worker #{} is initialised and ready to start", self.id);
                        }
                        Commands::Start(ids) => {
                            for id in ids.iter() {
                                if id == &self.id {
                                    // TODO: make random request to the API
                                    println!("Worker #{} is starting it's job", self.id);
                                    self.active = true;
                                    break;
                                }
                            }
                        }
                        Commands::Stop(ids) => {
                            for id in ids.iter() {
                                if id == &self.id {
                                    return;
                                }
                            }
                        }
                    }
                }
            } else {
                if counter == 0 {
                    println!("Cannot read data from channel");
                    return;
                }
                counter -= 1;

                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }

            if self.active {
                println!("Worker #{} is sending API request", self.id);
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            }
        }
    }
}

pub async fn run_performance_tests(num_of_threads: usize, test_duration: u64, load_profile: LoadProfile) {
    let (tx, mut rx) = watch::channel(Commands::Init);

    let mut set = JoinSet::new();

    for id in 0..num_of_threads {
        set.spawn({
            let mut worker = Worker::new(id as u32, rx.clone(), "http".to_string());
            async move {
                worker.run().await;
            }
        });
    }

    let ids: Vec<usize> = (0..num_of_threads).collect();
    let ids: Vec<u32> = ids.iter().map(|x| *x as u32).collect();
    tx.send(Commands::Start(ids.clone())).unwrap();

    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    tx.send(Commands::Stop(ids)).unwrap();

    while let Some(task) = set.join_next().await {
        match task {
            Ok(_) => {}
            Err(err) if err.is_panic() => {
                let err = err.into_panic();
                println!("Task panic: {:?}", err);
            }
            Err(err) => {
                let err = err.to_string();
                println!("Task error: {}", err);
            }
        }
    }
}