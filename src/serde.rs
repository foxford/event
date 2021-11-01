use std::ops::Bound;

use chrono::{DateTime, Utc};

type BoundedDatetimeTuple = (Bound<DateTime<Utc>>, Bound<DateTime<Utc>>);

pub(crate) mod ts_seconds_bound_tuple {
    use std::fmt;
    use std::ops::Bound;

    use super::BoundedDatetimeTuple;
    use chrono::{DateTime, NaiveDateTime, Utc};
    use serde::{de, ser};

    pub(crate) fn serialize<S>(
        value: &(Bound<DateTime<Utc>>, Bound<DateTime<Utc>>),
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        use ser::SerializeTuple;

        let (lt, rt) = value;
        let mut tup = serializer.serialize_tuple(2)?;

        match lt {
            Bound::Included(lt) => {
                let val = lt.timestamp();
                tup.serialize_element(&val)?;
            }
            Bound::Excluded(lt) => {
                // Adjusting the range to '[lt, rt)'
                let val = lt.timestamp() + 1;
                tup.serialize_element(&val)?;
            }
            Bound::Unbounded => {
                let val: Option<i64> = None;
                tup.serialize_element(&val)?;
            }
        }

        match rt {
            Bound::Included(rt) => {
                // Adjusting the range to '[lt, rt)'
                let val = rt.timestamp() - 1;
                tup.serialize_element(&val)?;
            }
            Bound::Excluded(rt) => {
                let val = rt.timestamp();
                tup.serialize_element(&val)?;
            }
            Bound::Unbounded => {
                let val: Option<i64> = None;
                tup.serialize_element(&val)?;
            }
        }

        tup.end()
    }

    pub fn deserialize<'de, D>(d: D) -> Result<BoundedDatetimeTuple, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        d.deserialize_tuple(2, TupleSecondsTimestampVisitor)
    }

    struct TupleSecondsTimestampVisitor;

    impl<'de> de::Visitor<'de> for TupleSecondsTimestampVisitor {
        type Value = BoundedDatetimeTuple;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a [lt, rt) range of unix time (seconds) or null (unbounded)")
        }

        /// Deserialize a tuple of two Bounded DateTime<Utc>
        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let lt = match seq.next_element()? {
                Some(Some(val)) => {
                    let dt = DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(val, 0), Utc);
                    Bound::Included(dt)
                }
                Some(None) => Bound::Unbounded,
                None => return Err(de::Error::invalid_length(1, &self)),
            };

            let rt = match seq.next_element()? {
                Some(Some(val)) => {
                    let dt = DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(val, 0), Utc);
                    Bound::Excluded(dt)
                }
                Some(None) => Bound::Unbounded,
                None => return Err(de::Error::invalid_length(2, &self)),
            };

            Ok((lt, rt))
        }
    }
}

///////////////////////////////////////////////////////////////////////////////

pub(crate) mod milliseconds_bound_tuples {
    use std::fmt;
    use std::ops::Bound;

    use serde::{de, ser};

    pub(crate) fn serialize<S>(
        value: &[(Bound<i64>, Bound<i64>)],
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        use ser::SerializeSeq;

        let mut seq = serializer.serialize_seq(Some(value.len()))?;

        for (lt, rt) in value {
            let lt = match lt {
                Bound::Included(lt) | Bound::Excluded(lt) => Some(lt),
                Bound::Unbounded => None,
            };

            let rt = match rt {
                Bound::Included(rt) | Bound::Excluded(rt) => Some(rt),
                Bound::Unbounded => None,
            };

            seq.serialize_element(&(lt, rt))?;
        }

        seq.end()
    }

    type BoundedI64Tuple = (Bound<i64>, Bound<i64>);
    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<Vec<BoundedI64Tuple>, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        pub struct MillisecondsBoundTupleVisitor;

        impl<'de> de::Visitor<'de> for MillisecondsBoundTupleVisitor {
            type Value = Vec<(Bound<i64>, Bound<i64>)>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a list of [lt, rt) range of integer milliseconds")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut elements: Self::Value = vec![];

                while let Some((Some(lt), Some(rt))) = seq.next_element()? {
                    if lt <= rt {
                        elements.push((Bound::Included(lt), Bound::Excluded(rt)))
                    } else {
                        return Err(de::Error::invalid_value(
                            de::Unexpected::Str(&format!("[{}, {}]", lt, rt)),
                            &"lt <= rt",
                        ));
                    }
                }

                Ok(elements)
            }
        }

        deserializer.deserialize_seq(MillisecondsBoundTupleVisitor)
    }
}

///////////////////////////////////////////////////////////////////////////////

pub(crate) mod ts_seconds_option_bound_tuple {
    use serde::de;
    use std::fmt;

    use super::BoundedDatetimeTuple;

    pub fn deserialize<'de, D>(d: D) -> Result<Option<BoundedDatetimeTuple>, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        d.deserialize_option(TupleSecondsTimestampVisitor)
    }

    pub struct TupleSecondsTimestampVisitor;

    impl<'de> de::Visitor<'de> for TupleSecondsTimestampVisitor {
        type Value = Option<BoundedDatetimeTuple>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter
                .write_str("none or a [lt, rt) range of unix time (seconds) or null (unbounded)")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, d: D) -> Result<Self::Value, D::Error>
        where
            D: de::Deserializer<'de>,
        {
            let interval = super::ts_seconds_bound_tuple::deserialize(d)?;
            Ok(Some(interval))
        }

        // We need this to deserialize json's nulls when payload structs are flattened into requests structs
        // Serde doesnt convert null to None thus we need to implement this
        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
    }
}

//////////////////////////////////////////////////////////////////////////////

pub(crate) mod duration_seconds {
    use std::fmt;

    use chrono::Duration;
    use serde::de;

    pub fn deserialize<'de, D>(d: D) -> Result<Duration, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        d.deserialize_u64(SecondsDurationVisitor)
    }

    pub struct SecondsDurationVisitor;

    impl<'de> de::Visitor<'de> for SecondsDurationVisitor {
        type Value = Duration;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("duration (seconds)")
        }

        fn visit_u64<E>(self, seconds: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Duration::seconds(seconds as i64))
        }
    }
}

///////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod test {
    use std::ops::Bound;

    use chrono::{DateTime, Duration, NaiveDateTime, Utc};
    use serde_derive::{Deserialize, Serialize};
    use serde_json::json;

    #[test]
    fn serialize_milliseconds_bound_tuples() {
        #[derive(Serialize)]
        struct Data {
            #[serde(with = "crate::serde::milliseconds_bound_tuples")]
            segments: Vec<(Bound<i64>, Bound<i64>)>,
        }

        let data = Data {
            segments: vec![
                (Bound::Included(0), Bound::Excluded(1000)),
                (Bound::Included(2000), Bound::Excluded(3000)),
            ],
        };

        let serialized = serde_json::to_string(&data).expect("Failed to serialize test data");
        let expected = r#"{"segments":[[0,1000],[2000,3000]]}"#;
        assert_eq!(serialized, expected);
    }

    #[test]
    fn deserialize_milliseconds_bound_tuples() {
        #[derive(Deserialize)]
        struct Data {
            #[serde(with = "crate::serde::milliseconds_bound_tuples")]
            segments: Vec<(Bound<i64>, Bound<i64>)>,
        }

        let data = serde_json::from_str::<Data>(r#"{"segments": [[0, 1000], [2000, 3000]]}"#)
            .expect("Failed to deserialize test data");

        let expected = vec![
            (Bound::Included(0), Bound::Excluded(1000)),
            (Bound::Included(2000), Bound::Excluded(3000)),
        ];

        assert_eq!(data.segments, expected);
    }

    #[derive(Debug, Deserialize)]
    struct TestOptionData {
        #[serde(default)]
        #[serde(with = "crate::serde::ts_seconds_option_bound_tuple")]
        time: Option<(Bound<DateTime<Utc>>, Bound<DateTime<Utc>>)>,
    }

    #[derive(Debug, Deserialize)]
    struct NestedTestOptionData {
        v: Option<String>,
        #[serde(flatten)]
        d: TestOptionData
    }

    #[test]
    fn ts_seconds_option_bound_tuple() {
        let now = now();

        let val = json!({
            "time": (now.timestamp(), now.timestamp()),
        });

        let data: TestOptionData = dbg!(serde_json::from_value(val).unwrap());
        let (start, end) = data.time.unwrap();
        assert_eq!(start, Bound::Included(now));
        assert_eq!(end, Bound::Excluded(now));

        let val = json!({});
        let data: TestOptionData = dbg!(serde_json::from_value(val).unwrap());
        assert!(data.time.is_none());

        let data: TestOptionData = dbg!(serde_json::from_str(r#"
        {
            "time": null
        }
        "#).unwrap());
        assert!(data.time.is_none());

        // These two tests should catch troubles with nulls in flattened structs
        let data: NestedTestOptionData = dbg!(serde_json::from_str(r#"
        {
            "v": "whatever",
            "time": null
        }
        "#).unwrap());
        assert!(data.d.time.is_none());

        let data: NestedTestOptionData = dbg!(serde_json::from_str(r#"
        {
            "v": "whatever"
        }
        "#).unwrap());
        assert!(data.d.time.is_none());
    }

    fn now() -> DateTime<Utc> {
        let now = Utc::now();
        let now = NaiveDateTime::from_timestamp(now.timestamp(), 0);
        DateTime::from_utc(now, Utc)
    }

    #[derive(Debug, Deserialize)]
    struct TestSecondsDurationData {
        #[serde(with = "crate::serde::duration_seconds")]
        duration: Duration,
    }

    #[test]
    fn duration_seconds() {
        let val = json!({"duration": 123});
        let data: TestSecondsDurationData = dbg!(serde_json::from_value(val).unwrap());
        assert_eq!(data.duration, Duration::seconds(123))
    }
}
