ALTER TABLE event ALTER COLUMN created_at DROP DEFAULT;

CREATE OR REPLACE FUNCTION on_event_insert() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    original RECORD;
BEGIN
    -- Let a user disable this trigger per session with `SET cfg.path_s3_upload = 'TRUE'`
    -- Was used for path to svg conversion
    IF current_setting('cfg.path_s3_upload', 't') = 'TRUE' THEN
        RETURN NEW;
    END IF;

    -- Blocks insert if there's concurrent insert into the same (room_id, set, label)
    -- tuple to avoid the race between original event and the next one
    PERFORM pg_advisory_xact_lock(hashtext(concat(NEW.room_id, NEW.set, NEW.label)));

    SELECT INTO original *
    FROM event
    WHERE deleted_at IS NULL
    AND   room_id = NEW.room_id
    AND   set = NEW.set
    AND   label = NEW.label
    ORDER BY created_at
    LIMIT 1;

    NEW.original_occurred_at := COALESCE(original.occurred_at, NEW.occurred_at);
    NEW.original_created_by := COALESCE(original.created_by, NEW.created_by);
    -- 'COALESCE' is used to allow setting custom 'created_at' values (e.g. for tests)
    -- `greatest` avoids creating original and non-original events with the same
    -- timestamp, so that 'original' event (the earliest one) never changes
    NEW.created_at = COALESCE(NEW.created_at, greatest(now(), original.created_at + '1 microsecond'));

    RETURN NEW;
END;
$$;
