ALTER TABLE event
ADD CONSTRAINT data_size
CHECK (
    length(data::text) < 102400
);
