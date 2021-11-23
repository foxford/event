CREATE OR REPLACE FUNCTION on_event_insert() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    original_occurred_at BIGINT;
BEGIN
    -- Let a user disable this trigger per session with `SET cfg.path_s3_upload = 'TRUE'`
    -- Was used for path to svg conversion
    IF current_setting('cfg.path_s3_upload', 't') = 'TRUE' THEN
        RETURN NEW;
    END IF;
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
