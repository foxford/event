use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PostcardBin<S> {
    value: S,
}

impl<S> PostcardBin<S> {
    pub fn new(value: S) -> Self {
        Self { value }
    }

    pub fn into_inner(self) -> S {
        self.value
    }
}

impl<S: Serialize> sqlx::Encode<'_, sqlx::Postgres> for PostcardBin<S> {
    fn encode_by_ref(
        &self,
        buf: &mut <sqlx::Postgres as sqlx::database::HasArguments>::ArgumentBuffer,
    ) -> sqlx::encode::IsNull {
        let encoded =
            postcard::to_allocvec(&self.value).expect("failed to encode as postcard binary");

        encoded.encode_by_ref(buf)
    }
}

impl<B: DeserializeOwned> sqlx::Decode<'_, sqlx::Postgres> for PostcardBin<B> {
    fn decode(
        value: <sqlx::Postgres as sqlx::database::HasValueRef<'_>>::ValueRef,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        // can't decode bytea as &[u8]
        let bytes: Vec<u8> = sqlx::Decode::decode(value)?;

        let value: B = {
            postcard::from_bytes(&bytes)
                .map_err(|err| format!("failed to decode postcard binary: {err}"))?
        };

        Ok(PostcardBin::new(value))
    }
}

impl<B> sqlx::Type<sqlx::Postgres> for PostcardBin<B> {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        <Vec<u8> as sqlx::Type<sqlx::Postgres>>::type_info()
    }
}
