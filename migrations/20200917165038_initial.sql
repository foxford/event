CREATE EXTENSION IF NOT EXISTS pgcrypto;

DO $$ BEGIN
    CREATE TYPE account_id AS (
        label text,
        audience text
    );
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

DO $$ BEGIN
    CREATE TYPE agent_id AS (
        account_id account_id,
        label text
    );
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

DO $$ BEGIN
    CREATE TYPE agent_status AS ENUM ('in_progress', 'ready');
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

DO $$ BEGIN
    CREATE TYPE change_type AS ENUM ('addition', 'modification', 'removal');
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

--------------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS room (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    audience text NOT NULL,
    source_room_id uuid,
    "time" tstzrange NOT NULL,
    tags json,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT room_time_presence CHECK (("time" <> 'empty'::tstzrange)),

    PRIMARY KEY (id),
    FOREIGN KEY (source_room_id) REFERENCES room(id) ON DELETE SET NULL
);

--------------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS event (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    room_id uuid NOT NULL,
    kind text NOT NULL,
    set text NOT NULL,
    label text,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    occurred_at bigint NOT NULL,
    created_by agent_id NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    deleted_at timestamp with time zone,
    original_occurred_at bigint NOT NULL,

    PRIMARY KEY (id),
    FOREIGN KEY (room_id) REFERENCES room(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS event_kind_idx
ON event USING btree (kind)
WHERE (deleted_at IS NULL);

CREATE INDEX IF NOT EXISTS event_set_label_idx
ON event USING btree (set, label)
WHERE (deleted_at IS NULL);

CREATE INDEX IF NOT EXISTS event_state_idx
ON event USING btree (room_id, set, label, original_occurred_at, occurred_at)
WHERE deleted_at IS NULL;

CREATE OR REPLACE FUNCTION on_event_insert() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    original_occurred_at BIGINT;
BEGIN
    original_occurred_at := (
        SELECT occurred_at
        FROM event
        WHERE deleted_at IS NULL
        AND   room_id = NEW.room_id
        AND   set = NEW.set
        AND   label = NEW.label
        ORDER BY occurred_at
        LIMIT 1
    );

    NEW.original_occurred_at := COALESCE(original_occurred_at, NEW.occurred_at);
    RETURN NEW;
END;
$$;

DO $$ BEGIN
    CREATE TRIGGER event_insert_trigger BEFORE INSERT
    ON event FOR EACH ROW EXECUTE FUNCTION on_event_insert();
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

--------------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS adjustment (
    room_id uuid NOT NULL,
    started_at timestamp with time zone NOT NULL,
    segments int8range[] NOT NULL,
    "offset" bigint NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,

    PRIMARY KEY (room_id),
    FOREIGN KEY (room_id) REFERENCES room(id) ON DELETE CASCADE
);

--------------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS agent (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    agent_id agent_id NOT NULL,
    room_id uuid NOT NULL,
    status agent_status DEFAULT 'in_progress'::agent_status NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,

    PRIMARY KEY (id),
    UNIQUE (agent_id, room_id),
    FOREIGN KEY (room_id) REFERENCES room(id) ON DELETE CASCADE
);

--------------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS edition (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    source_room_id uuid NOT NULL,
    created_by agent_id NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,

    PRIMARY KEY (id),
    FOREIGN KEY (source_room_id) REFERENCES room(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS edition_source_room_id_idx ON edition USING btree (source_room_id);

--------------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS change (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    edition_id uuid NOT NULL,
    kind change_type NOT NULL,
    event_id uuid,
    event_kind text,
    event_set text,
    event_label text,
    event_data jsonb,
    event_occurred_at bigint,
    event_created_by agent_id,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT event_data_and_event_id_present CHECK (
        CASE kind
            WHEN 'addition'::change_type THEN ((event_id IS NULL) AND (event_kind IS NOT NULL) AND (event_data IS NOT NULL) AND (event_occurred_at IS NOT NULL) AND (event_created_by IS NOT NULL))
            WHEN 'modification'::change_type THEN ((event_id IS NOT NULL) AND ((event_kind IS NOT NULL) OR (event_set IS NOT NULL) OR (event_label IS NOT NULL) OR (event_data IS NOT NULL) OR (event_occurred_at IS NOT NULL) OR (event_created_by IS NOT NULL)))
            WHEN 'removal'::change_type THEN (event_id IS NOT NULL)
            ELSE NULL::boolean
        END
    ),

    PRIMARY KEY (id),
    FOREIGN KEY (edition_id) REFERENCES edition(id) ON DELETE CASCADE,
    FOREIGN KEY (event_id) REFERENCES event(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS change_edition_id_idx ON change USING btree (edition_id);

--------------------------------------------------------------------------------

DROP TABLE IF EXISTS __diesel_schema_migrations;
DROP FUNCTION IF EXISTS diesel_manage_updated_at;
DROP FUNCTION IF EXISTS diesel_set_updated_at;
