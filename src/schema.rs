table! {
    use diesel::sql_types::*;
    use crate::db::sql::*;

    adjustment (room_id) {
        room_id -> Uuid,
        started_at -> Timestamptz,
        segments -> Array<Int8range>,
        offset -> Int8,
        created_at -> Timestamptz,
    }
}

table! {
    use diesel::sql_types::*;
    use crate::db::sql::*;

    agent (id) {
        id -> Uuid,
        agent_id -> Agent_id,
        room_id -> Uuid,
        status -> Agent_status,
        created_at -> Timestamptz,
    }
}

table! {
    use diesel::sql_types::*;
    use crate::db::sql::*;

    event (id) {
        id -> Uuid,
        room_id -> Uuid,
        kind -> Text,
        set -> Text,
        label -> Nullable<Text>,
        data -> Jsonb,
        occured_at -> Int8,
        created_by -> Agent_id,
        created_at -> Timestamptz,
        deleted_at -> Nullable<Timestamptz>,
    }
}

table! {
    use diesel::sql_types::*;
    use crate::db::sql::*;

    room (id) {
        id -> Uuid,
        audience -> Text,
        source_room_id -> Nullable<Uuid>,
        time -> Tstzrange,
        tags -> Nullable<Json>,
        created_at -> Timestamptz,
    }
}

joinable!(adjustment -> room (room_id));
joinable!(agent -> room (room_id));
joinable!(event -> room (room_id));

allow_tables_to_appear_in_same_query!(
    adjustment,
    agent,
    event,
    room,
);
