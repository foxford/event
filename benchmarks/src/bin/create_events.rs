extern crate event_benchmark;

use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use clap::{value_t, App, Arg};
use futures::executor::ThreadPool;
use log::info;
use serde_derive::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use svc_agent::{
    mqtt::{compat, AgentBuilder, AgentConfig, ConnectionMode, Notification, QoS, ResponseStatus},
    AccountId, AgentId, Subscription,
};
use uuid::Uuid;

use event_benchmark::{Interval, TestAgent};

///////////////////////////////////////////////////////////////////////////////

const API_VERSION: &'static str = "v1";
const ROOM_DURATION: i64 = 3600;

#[derive(Debug, Serialize)]
struct RoomCreateRequest {
    audience: String,
    time: (i64, i64),
    tags: JsonValue,
}

#[derive(Debug, Deserialize)]
struct RoomCreateResponse {
    id: Uuid,
}

#[derive(Debug, Serialize)]
struct RoomEnterRequest {
    id: Uuid,
}

#[derive(Debug, Deserialize)]
struct RoomEnterResponse {}

#[derive(Clone, Debug, Serialize)]
struct EventCreateRequest {
    room_id: Uuid,
    #[serde(rename = "type")]
    kind: String,
    data: JsonValue,
}

///////////////////////////////////////////////////////////////////////////////

/// Listens to event.create events in all rooms and remembers their processing times.
struct StatsAgent {
    is_running: AtomicBool,
    processing_times: Mutex<Vec<usize>>,
}

impl StatsAgent {
    fn new() -> Self {
        Self {
            is_running: AtomicBool::new(false),
            processing_times: Mutex::new(vec![]),
        }
    }

    async fn start(&self, config: &AgentConfig, id: AgentId, service_account_id: &AccountId) {
        // Start agent.
        let (mut agent, rx) = AgentBuilder::new(id.clone(), API_VERSION)
            .connection_mode(ConnectionMode::Default)
            .start(&config)
            .expect("Failed to start agent");

        // Subscribe to the service responses.
        let subscription =
            Subscription::broadcast_events(service_account_id, API_VERSION, "rooms/+/events");

        agent
            .subscribe(&subscription, QoS::AtLeastOnce, None)
            .expect("Error subscribing to broadcast events");

        // Set running and wait for notifications until stop.
        self.is_running.store(true, Ordering::SeqCst);
        let mut processing_times = self.processing_times.lock().expect("Failed to obtain lock");

        while self.is_running.load(Ordering::SeqCst) {
            if let Ok(Notification::Publish(message)) = rx.try_recv() {
                // Parse event properties.
                let bytes = message.payload.as_slice();

                let envelope = serde_json::from_slice::<compat::IncomingEnvelope>(bytes)
                    .expect("Failed to parse incoming message");

                if let compat::IncomingEnvelopeProperties::Event(evp) = envelope.properties() {
                    // Remember processing time for event.create notification.
                    if evp.label() == Some("event.create") {
                        let evp_value =
                            serde_json::to_value(evp).expect("Failed to dump event properties");

                        let broker_timestamp = evp_value
                            .get("broker_timestamp")
                            .expect("Missing broker timestamp in event properties")
                            .as_str()
                            .expect("Failed to cast broker timestamp as string")
                            .parse::<usize>()
                            .expect("Failed to parse broker timestamp");

                        let initial_timestamp = evp_value
                            .get("initial_timestamp")
                            .expect("Missing initial timestamp in event properties")
                            .as_str()
                            .expect("Failed to cast initial timestamp as string")
                            .parse::<usize>()
                            .expect("Failed to parse initial timestamp");

                        processing_times.push(broker_timestamp - initial_timestamp);
                    }
                }
            }
        }
    }

    fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
    }

    fn with_processing_times<F, R>(&self, fun: F) -> R
    where
        F: FnOnce(&[usize]) -> R,
    {
        let processing_times = self
            .processing_times
            .lock()
            .expect("Failed to obtain lock because stats agent failed during data collection");

        fun(&*processing_times.as_slice())
    }
}

///////////////////////////////////////////////////////////////////////////////

fn main() {
    env_logger::init();

    let matches = App::new("event.create benchmark")
        .about("Creates N rooms with M online agents and makes R event.create RPS as each.")
        .arg(
            Arg::with_name("mqtt-host")
                .short("H")
                .long("mqtt-host")
                .help("Broker connection host")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("mqtt-port")
                .short("P")
                .long("mqtt-port")
                .help("Broker connection port")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("account-id")
                .short("A")
                .long("account-id")
                .help("Account ID with which agents connect")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("service-account-id")
                .short("S")
                .long("service-account-id")
                .help("Event service account ID")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("rooms")
                .short("r")
                .long("rooms")
                .help("Number of rooms")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("agents")
                .short("a")
                .long("agents")
                .help("Number of agents per room")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("rate")
                .short("R")
                .long("rate")
                .help("Number of event.create RPS to make by each agent")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("tag")
                .short("t")
                .long("tag")
                .help("Tag value to mark rooms for this run")
                .takes_value(true),
        )
        .get_matches();

    let mqtt_host = matches.value_of("mqtt-host").unwrap_or("0.0.0.0");
    let mqtt_port = value_t!(matches, "mqtt-port", u16).unwrap_or(1883);

    let account_id = AccountId::from_str(
        matches
            .value_of("account-id")
            .unwrap_or("test.dev.usr.example.org"),
    )
    .expect("Failed to parse account ID");

    let service_account_id = AccountId::from_str(
        matches
            .value_of("service-account-id")
            .unwrap_or("event.dev.svc.example.org"),
    )
    .expect("Failed to parse service account ID");

    let rooms_count = value_t!(matches, "rooms", usize).unwrap_or(1);
    let agents_per_room = value_t!(matches, "agents", usize).unwrap_or(1);
    let rate = value_t!(matches, "rate", u64).unwrap_or(1);
    let tag = matches.value_of("tag").unwrap_or("benchmark");

    ///////////////////////////////////////////////////////////////////////////

    let pool = Arc::new(ThreadPool::new().expect("Failed to build thread pool"));
    let agent_config_json = format!(
        "{{
            \"uri\": \"{}:{}\",
            \"incomming_message_queue_size\": 1000000,
            \"outgoing_message_queue_size\": 1000000
        }}",
        mqtt_host, mqtt_port
    );

    let agent_config = serde_json::from_str::<AgentConfig>(&agent_config_json)
        .expect("Failed to parse agent config");

    info!("Connecting stats agent.");
    let stats_agent_id = AgentId::new("stats", account_id.clone());
    let stats_agent = Arc::new(StatsAgent::new());
    let stats_agent_clone = stats_agent.clone();
    let agent_config_clone = agent_config.clone();
    let service_account_id_clone = service_account_id.clone();

    pool.spawn_ok(async move {
        stats_agent_clone
            .start(
                &agent_config_clone,
                stats_agent_id,
                &service_account_id_clone,
            )
            .await;
    });

    ///////////////////////////////////////////////////////////////////////////

    info!("Connecting {} agents.", rooms_count * agents_per_room);

    let rooms_agents = (0..rooms_count)
        .map(|room_idx| {
            (0..agents_per_room)
                .map(|agent_idx| {
                    // Start agent with service subscription.
                    let agent_label = format!("bench-{}-{}", room_idx, agent_idx);
                    let agent_id = AgentId::new(&agent_label, account_id.clone());
                    TestAgent::start(&agent_config, agent_id, pool.clone(), &service_account_id)
                })
                .collect::<Vec<TestAgent>>()
        })
        .collect::<Vec<Vec<TestAgent>>>();

    ///////////////////////////////////////////////////////////////////////////

    info!("Creating and entering {} rooms.", rooms_count);

    let registry = rooms_agents
        .into_iter()
        .map(|room_agents| {
            // Create room as the first agent.
            let now = Utc::now().timestamp();

            let response = room_agents[0].request::<RoomCreateRequest, RoomCreateResponse>(
                "room.create",
                RoomCreateRequest {
                    audience: account_id.audience().to_owned(),
                    time: (now, now + ROOM_DURATION),
                    tags: json!({ "benchmark": tag }),
                },
            );

            if response.properties().status() != ResponseStatus::CREATED {
                panic!("Failed to create room");
            }

            let room_id = response.payload().id;

            // Enter room as each agent.
            for agent in &room_agents {
                let response = agent.request::<RoomEnterRequest, RoomEnterResponse>(
                    "room.enter",
                    RoomEnterRequest { id: room_id },
                );

                if response.properties().status() != ResponseStatus::ACCEPTED {
                    panic!("Failed to enter room");
                }
            }

            (room_id, room_agents)
        })
        .collect::<Vec<(Uuid, Vec<TestAgent>)>>();

    ///////////////////////////////////////////////////////////////////////////

    info!(
        "Creating {} events per second for each agent. Stop with Ctrl+C.",
        rate
    );
    let mut intervals = Vec::with_capacity(rooms_count * agents_per_room);

    // Run event creation loop for each agent in each room.
    for (room_id, agents) in registry {
        for agent in agents {
            let interval = Arc::new(Interval::new(rate));
            let interval_clone = interval.clone();

            pool.spawn_ok(async move {
                interval_clone
                    .run(|| {
                        info!("Create event");

                        agent.request_nowait(
                            "event.create",
                            EventCreateRequest {
                                room_id,
                                kind: String::from("message"),
                                data: json!({ "text": "hello" }),
                            },
                        )
                    })
                    .await;
            });

            intervals.push(interval);
        }
    }

    ///////////////////////////////////////////////////////////////////////////

    // Wait for Ctrl + C to stop.
    let is_running = Arc::new(AtomicBool::new(true));
    let is_running_clone = is_running.clone();

    ctrlc::set_handler(move || {
        for interval in &intervals {
            interval.stop();
        }

        is_running_clone.store(false, Ordering::SeqCst);
    })
    .expect("Failed to set Ctrl+C handler");

    while is_running.load(Ordering::SeqCst) {}

    ///////////////////////////////////////////////////////////////////////////

    // Stop collecting stats to unlock.
    stats_agent.stop();

    // Calculate and print results.
    stats_agent.with_processing_times(|times| {
        if times.len() > 0 {
            let avg = times.iter().fold(0, |acc, x| acc + x) / times.len();
            println!("Average processing time: {}", avg);
        } else {
            println!("No events captured");
        }
    });
}
