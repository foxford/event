create type agent_status as ENUM ('in_progress', 'ready');

create table agent (
    id uuid default gen_random_uuid(),
    agent_id agent_id not null,
    room_id uuid not null,
    status agent_status not null default 'in_progress',
    created_at timestamptz not null default now(),

    foreign key (room_id) references room (id) on delete cascade,
    unique (agent_id, room_id),
    primary key (id)
);
