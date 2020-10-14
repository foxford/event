ALTER TABLE event
ADD CONSTRAINT data_size
CHECK (
    deleted_at IS NOT NULL OR length(data::text) < 102400
);
