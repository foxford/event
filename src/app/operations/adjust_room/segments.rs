use crate::db::event::Object as Event;
use anyhow::Result;
use chrono::Duration;
use std::ops::Bound;
use std::str::FromStr;
use std::time::Duration as StdDuration;
use svc_agent::AgentId;

const NS_IN_MS: i64 = 1_000_000;

/// Turns `segments` into gaps.
pub fn invert_segments(
    segments: &[(i64, i64)],
    room_duration: Duration,
    min_segment_length: StdDuration,
) -> Result<Vec<(i64, i64)>> {
    if segments.is_empty() {
        let total_nanos = room_duration.num_nanoseconds().unwrap_or(std::i64::MAX);
        return Ok(vec![(0, total_nanos)]);
    }

    let mut gaps = Vec::with_capacity(segments.len() + 2);

    // A possible gap before the first segment.
    if let Some((first_segment_start, _)) = segments.first() {
        if *first_segment_start > 0 {
            gaps.push((0, *first_segment_start));
        }
    }

    // Gaps between segments.
    for ((_, segment_stop), (next_segment_start, _)) in segments.iter().zip(&segments[1..]) {
        gaps.push((*segment_stop, *next_segment_start));
    }

    // A possible gap after the last segment.
    if let Some((_, last_segment_stop)) = segments.last() {
        let room_duration_nanos = room_duration.num_nanoseconds().unwrap_or(std::i64::MAX);

        // Don't create segments less than `min_segment_length`
        if *last_segment_stop < room_duration_nanos
            && StdDuration::from_nanos((room_duration_nanos - last_segment_stop) as u64)
                .gt(&min_segment_length)
        {
            gaps.push((*last_segment_stop, room_duration_nanos));
        }
    }

    Ok(gaps)
}

pub fn collect_pin_segments(
    pin_events: &[Event],
    event_room_offset: Duration,
    recording_created_by: &AgentId,
    recording_end: i64,
) -> Vec<(Bound<i64>, Bound<i64>)> {
    let mut pin_segments = vec![];
    let mut pin_start = None;

    let mut add_segment = |start, end| {
        if start <= end && start >= 0 && end <= recording_end {
            pin_segments.push((Bound::Included(start), Bound::Excluded(end)));
        }
    };

    for event in pin_events {
        if let Some(agent_id) = event
            .data()
            .get("agent_id")
            .and_then(|v| v.as_str())
            .and_then(|v| AgentId::from_str(v).ok())
        {
            // Shift from the event room's dimension to the recording's dimension.
            let occurred_at = event.occurred_at() / NS_IN_MS - event_room_offset.num_milliseconds();

            if agent_id == *recording_created_by {
                // Stream was pinned.
                // Its possible that teacher pins someone twice in a row
                // Do nothing in that case
                if pin_start.is_none() {
                    pin_start = Some(occurred_at);
                }
            } else if let Some(pinned_at) = pin_start {
                // stream was unpinned
                // its possible that pinned_at equals unpin's occurred_at after adjust
                // we skip segments like that
                if occurred_at > pinned_at {
                    add_segment(pinned_at, occurred_at);
                }
                pin_start = None;
            }
        }
    }

    // If the stream hasn't got unpinned since some moment then add a pin segment to the end
    // of the recording to keep it pinned.
    if let Some(start) = pin_start {
        add_segment(start, recording_end);
    }

    pin_segments
}

#[cfg(test)]
mod tests {
    mod collect_pinned_events {
        use chrono::{DateTime, Duration};
        use std::ops::Bound;
        use std::str::FromStr;
        use svc_agent::AgentId;
        use uuid::Uuid;

        use crate::app::operations::adjust_room::v2::PIN_EVENT_TYPE;
        use crate::db::event::Builder as EventBuilder;

        use super::super::*;

        #[derive(Clone, Debug, serde::Serialize)]
        pub struct PinEventData {
            agent_id: Option<AgentId>,
        }

        impl PinEventData {
            pub fn new(agent_id: AgentId) -> Self {
                Self {
                    agent_id: Some(agent_id),
                }
            }

            pub fn null() -> Self {
                Self { agent_id: None }
            }

            fn to_json(self) -> serde_json::Value {
                serde_json::to_value(self).unwrap()
            }
        }

        #[test]
        fn test_collect_pinned_events_cons_pin() {
            let agent_id = AgentId::from_str("web.agent.usr.foobar").unwrap();
            let event_room_id = Uuid::new_v4();

            // This should produce single pin segment between earliest pin and (latest) unpin
            // "Repin" of the same agent should be skipped
            let xs = vec![
                EventBuilder::new()
                    .room_id(event_room_id)
                    .set(PIN_EVENT_TYPE)
                    .data(&PinEventData::new(agent_id.to_owned()).to_json())
                    .occurred_at(2_041_237_463_815)
                    .build()
                    .unwrap(),
                EventBuilder::new()
                    .room_id(event_room_id)
                    .set(PIN_EVENT_TYPE)
                    .data(&PinEventData::new(agent_id.to_owned()).to_json())
                    .occurred_at(2_041_238_581_600)
                    .build()
                    .unwrap(),
                EventBuilder::new()
                    .room_id(event_room_id)
                    .set(PIN_EVENT_TYPE)
                    .data(&PinEventData::null().to_json())
                    .occurred_at(2_093_817_792_770)
                    .build()
                    .unwrap(),
            ];

            let pin_segments =
                collect_pin_segments(&xs, Duration::seconds(0), &agent_id, 2 * 2093817792770);

            assert_eq!(pin_segments.len(), 1);
            let seg = pin_segments[0];

            if let Bound::Included(s) = seg.0 {
                if let Bound::Excluded(e) = seg.1 {
                    assert_eq!(s, 2041237);
                    assert_eq!(e, 2093817);
                    return;
                }
            }

            unreachable!("Patterns above were expected to match");
        }

        #[test]
        fn test_collect_pinned_events_fast_pin_unpin() {
            let agent_id = AgentId::from_str("web.agent.usr.foobar").unwrap();
            let event_room_id = Uuid::new_v4();

            // This should produce no pins since unpin happened just after pin
            let xs = vec![
                EventBuilder::new()
                    .room_id(event_room_id)
                    .set(PIN_EVENT_TYPE)
                    .data(&PinEventData::new(agent_id.to_owned()).to_json())
                    .occurred_at(3312020_000_001)
                    .build()
                    .unwrap(),
                EventBuilder::new()
                    .room_id(event_room_id)
                    .set(PIN_EVENT_TYPE)
                    .data(&PinEventData::null().to_json())
                    .occurred_at(3312020_000_003)
                    .build()
                    .unwrap(),
            ];

            let pin_segments =
                collect_pin_segments(&xs, Duration::seconds(0), &agent_id, 2 * 2093817792770);

            assert_eq!(dbg!(pin_segments).len(), 0);
        }

        #[test]
        fn test_invalid_pin_segment_after_video() {
            let agent_id = AgentId::from_str("web.agent.usr.foobar").unwrap();
            let event_room_id = Uuid::new_v4();

            let xs = vec![EventBuilder::new()
                .room_id(event_room_id)
                .set(PIN_EVENT_TYPE)
                .data(&PinEventData::new(agent_id.to_owned()).to_json())
                // after segment
                .occurred_at(3670995397747)
                .build()
                .unwrap()];

            let event_room_offset = DateTime::parse_from_rfc3339("2022-09-05T09:52:44.162+03:00")
                .unwrap()
                - (DateTime::parse_from_rfc3339("2022-09-05T10:00:28.802+03:00").unwrap()
                    - Duration::milliseconds(4018));

            let pin_segments = collect_pin_segments(&xs, event_room_offset, &agent_id, 2943194);
            // no pin segments
            assert_eq!(pin_segments.len(), 0);
        }
    }
}
