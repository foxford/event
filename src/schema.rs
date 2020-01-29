table! {
    adjustment (room_id) {
        room_id -> Uuid,
        started_at -> Timestamptz,
        segments -> Array<Int8range>,
        offset -> Int8,
        created_at -> Timestamptz,
    }
}

table! {
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

allow_tables_to_appear_in_same_query!(adjustment, room,);
