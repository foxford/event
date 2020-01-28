create table room (
    id uuid default gen_random_uuid(),

    time tstzrange not null,
    audience text not null,
    backend_room_id uuid not null,
    tags json null,
    fragments int8range[] null,
    preroll_duration bigint null,
    created_at timestamptz not null default now(),

    primary key (id)
);
