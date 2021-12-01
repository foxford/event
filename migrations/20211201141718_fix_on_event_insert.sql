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
    SELECT INTO original *
    FROM event
    WHERE deleted_at IS NULL
    AND   room_id = NEW.room_id
    AND   set = NEW.set
    AND   label = NEW.label
    ORDER BY occurred_at
    LIMIT 1;

    NEW.original_occurred_at := COALESCE(original.occurred_at, NEW.occurred_at);
    NEW.original_created_by := COALESCE(original.created_by, NEW.created_by);
    RETURN NEW;
END;
$$;
