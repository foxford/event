use std::convert::TryFrom;
use std::ops::Bound;

use chrono::{DateTime, Utc};
use serde_derive::{Deserialize, Serialize};

pub type BoundedDateTimeTuple = (Bound<DateTime<Utc>>, Bound<DateTime<Utc>>);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum RoomTimeBound {
    Excluded(DateTime<Utc>),
    Unbounded,
}

impl From<RoomTimeBound> for Bound<DateTime<Utc>> {
    fn from(bound: RoomTimeBound) -> Self {
        match bound {
            RoomTimeBound::Excluded(dt) => Bound::Excluded(dt),
            RoomTimeBound::Unbounded => Bound::Unbounded,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(try_from = "BoundedDateTimeTuple")]
#[serde(into = "BoundedDateTimeTuple")]
pub struct RoomTime {
    start: DateTime<Utc>,
    end: RoomTimeBound,
}

impl RoomTime {
    pub fn new(tuple: impl Into<BoundedDateTimeTuple>) -> Option<Self> {
        match tuple.into() {
            (Bound::Included(start), Bound::Excluded(end)) if start < end => Some(Self {
                start,
                end: RoomTimeBound::Excluded(end),
            }),
            (Bound::Included(start), Bound::Unbounded) => Some(Self {
                start,
                end: RoomTimeBound::Unbounded,
            }),
            _ => None,
        }
    }

    pub fn update(&self, tuple: BoundedDateTimeTuple) -> Option<Self> {
        let now = Utc::now();
        let new_room_time = Self::new(tuple);
        if new_room_time.is_none() {
            return None;
        }

        let Self {
            start: new_start,
            end: new_end,
        } = new_room_time.unwrap();
        match (self.start, &self.end) {
            (s, _) if s > now => {
                return Some(Self {
                    start: new_start,
                    end: new_end,
                });
            }
            (_, RoomTimeBound::Excluded(e)) if *e < now => {
                return None;
            }
            _ => match new_end {
                RoomTimeBound::Excluded(e) if e > now => {
                    return Some(Self {
                        start: self.start,
                        end: new_end,
                    });
                }
                RoomTimeBound::Unbounded => {
                    return Some(Self {
                        start: self.start,
                        end: new_end,
                    });
                }
                RoomTimeBound::Excluded(_) => {
                    return Some(Self {
                        start: self.start,
                        end: RoomTimeBound::Excluded(now),
                    });
                }
            },
        }
    }

    pub fn start(&self) -> &DateTime<Utc> {
        &self.start
    }

    pub fn end(&self) -> &RoomTimeBound {
        &self.end
    }
}

impl From<RoomTime> for BoundedDateTimeTuple {
    fn from(time: RoomTime) -> BoundedDateTimeTuple {
        match time.end {
            RoomTimeBound::Unbounded => (Bound::Included(time.start), Bound::Unbounded),
            RoomTimeBound::Excluded(end) => (Bound::Included(time.start), Bound::Excluded(end)),
        }
    }
}

impl TryFrom<BoundedDateTimeTuple> for RoomTime {
    type Error = String;

    fn try_from(t: BoundedDateTimeTuple) -> Result<Self, Self::Error> {
        match RoomTime::new((t.0, t.1)) {
            Some(rt) => Ok(rt),
            None => Err(format!("Invalid room time: ({:?}, {:?})", t.0, t.1)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as Dur;
    use Bound::Excluded as Bex;
    use Bound::Included as Bin;

    #[test]
    fn test_update_past_room() {
        // cant move past room at all
        let now = Utc::now();
        let rt =
            RoomTime::new((Bin(now - Dur::hours(3)), Bex(now - Dur::hours(2)))).expect("Cant fail");

        // move to past
        assert_eq!(
            rt.update((Bin(now - Dur::hours(5)), Bex(now - Dur::hours(1)))),
            None
        );

        // move to present
        assert_eq!(
            rt.update((Bin(now - Dur::hours(5)), Bex(now + Dur::hours(1)))),
            None
        );

        // move to future
        assert_eq!(
            rt.update((Bin(now + Dur::hours(5)), Bex(now + Dur::hours(8)))),
            None
        );
    }

    #[test]
    fn test_update_future_room() {
        // can move future room anywhere
        let now = Utc::now();
        let rt =
            RoomTime::new((Bin(now + Dur::hours(3)), Bex(now + Dur::hours(4)))).expect("Cant fail");

        // move to past
        assert_eq!(
            rt.update((Bin(now - Dur::hours(5)), Bex(now - Dur::hours(1))))
                .is_some(),
            true
        );

        // move to present
        assert_eq!(
            rt.update((Bin(now - Dur::hours(5)), Bex(now + Dur::hours(1))))
                .is_some(),
            true
        );

        // move to future
        assert_eq!(
            rt.update((Bin(now + Dur::hours(5)), Bex(now + Dur::hours(8))))
                .is_some(),
            true
        );
    }

    #[test]
    fn test_update_present_room() {
        // present room open dt never moves and close dt is at least 'now'
        let now = Utc::now();
        let old_start = Bin(now - Dur::hours(3));
        let old_end = Bex(now + Dur::hours(4));
        let rt = RoomTime::new((old_start, old_end)).expect("Cant fail");

        let rt1: BoundedDateTimeTuple = rt
            .update((Bin(now - Dur::hours(5)), Bex(now - Dur::hours(1))))
            .expect("Shouldnt fail")
            .into();
        // move to past
        assert_eq!(rt1.0, old_start);
        if let Bex(end) = rt1.1 {
            assert!((end - now).num_seconds().abs() < 1);
        } else {
            panic!("Invalid Bound");
        }

        // move to present
        let rt2: BoundedDateTimeTuple = rt
            .update((Bin(now - Dur::hours(5)), Bex(now + Dur::hours(8))))
            .expect("Shouldnt fail")
            .into();
        assert_eq!(rt2.0, old_start);
        assert_eq!(rt2.1, Bex(now + Dur::hours(8)));

        // move to future
        let rt3: BoundedDateTimeTuple = rt
            .update((Bin(now + Dur::hours(5)), Bex(now + Dur::hours(8))))
            .expect("Shouldnt fail")
            .into();
        assert_eq!(rt3.0, old_start);
        assert_eq!(rt3.1, Bex(now + Dur::hours(8)));
    }
}
