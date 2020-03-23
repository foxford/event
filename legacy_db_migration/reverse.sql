begin;

-- Connect to the new DB.
create extension if not exists "dblink";
create foreign data wrapper new_db_wrapper validator postgresql_fdw_validator;
create server new_db foreign data wrapper new_db_wrapper options (hostaddr '${TARGET_HOST}', port '${TARGET_PORT}', dbname '${TARGET_DB}');
create user mapping for postgres server new_db options (user '${TARGET_USER}', password '${TARGET_PASSWORD}');
select dblink_connect('new_db');
grant usage on foreign server new_db to postgres;

-- Add missing rooms and update existing rooms' open/close dates & stream metadata.
insert into rooms (
    id,
    created_at,
    opened_at,
    closed_at,
    audience,
    account_id,
    agent_label,
    parent_id,
    stream
)
select
    id,
    created_at,
    lower(time) as opened_at,
    adjusted_at as closed_at,
    audience,
    'portal' as account_id,
    '' as agent_label,
    source_room_id as parent_id,
    case adjusted_at is null
        when true then '{}'::jsonb
        else jsonb_build_object(
            'preroll', "offset",
            'fragments', (
                select to_jsonb(array_agg(array[l, u]))
                from (
                    select lower(s) as l, upper(s) as u
                    from unnest(segments) as s
                ) as q
            )
        )
    end as stream
from dblink('new_db', '
    select
        r.id,
        r.audience,
        r.source_room_id,
        r.time,
        r.created_at,
        a.started_at,
        a.segments,
        a."offset",
        a.created_at as adjusted_at
    from room as r
    left join adjustment as a
    on a.room_id = r.id
') as data(
    id uuid,
    audience text,
    source_room_id uuid,
    time tstzrange,
    created_at timestamptz,
    started_at timestamptz,
    segments int8range[],
    "offset" bigint,
    adjusted_at timestamptz
)
on conflict (id)
do update
set opened_at = excluded.opened_at,
    closed_at = excluded.closed_at;

-- Add missing events.
insert into events (
    id,
    type,
    room_id,
    created_at,
    data,
    audience,
    account_id,
    agent_label,
    "offset"
)
select
    id,
    kind as type,
    room_id,
    created_at,
    case kind = 'draw' and doc_data is not null
        when true then jsonb_build_object(
            'url', doc_data->'url',
            'page', case set like 'draw_%'
                when true then substring(set, length(set) - strpos(reverse(set), '_') + 2)::bigint
                else null
            end,
            'geometry', data
        )
        else data
    end as data,
    audience,
    account_label as account_id,
    agent_label,
    occurred_at / 1000000 as "offset"
from dblink('new_db', '
    select
        e.id,
        e.kind,
        e.set,
        e.room_id,
        e.created_at,
        e.data,
        (e.created_by).label as agent_label,
        (e.created_by).account_id.label as account_label,
        (e.created_by).account_id.audience as audience,
        e.occurred_at,
        case e.kind = ''draw''
            when true then (
                select d.data
                from event as d
                where d.kind = ''document''
                and   e.set like (''draw_'' || d.label || ''_%'')
                limit 1
            )
            else null
        end as doc_data
    from event as e
') as data(
    id uuid,
    kind text,
    set text,
    room_id uuid,
    created_at timestamptz,
    data jsonb,
    agent_label text,
    account_label text,
    audience text,
    occurred_at bigint,
    doc_data jsonb
)
on conflict (id)
do nothing;

-- Cleanup.
drop server new_db cascade;
drop foreign data wrapper new_db_wrapper;
drop extension "dblink";

commit;
