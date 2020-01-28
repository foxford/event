table! {
    room (id) {
        id -> Uuid,
        time -> Tstzrange,
        audience -> Text,
        backend_room_id -> Uuid,
        tags -> Nullable<Json>,
        fragments -> Nullable<Array<Int8range>>,
        preroll_duration -> Nullable<Int8>,
        created_at -> Timestamptz,
    }
}
