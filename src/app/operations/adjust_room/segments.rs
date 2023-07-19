use anyhow::Result;
use chrono::Duration;
use std::time::Duration as StdDuration;

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
