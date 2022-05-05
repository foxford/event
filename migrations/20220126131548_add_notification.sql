CREATE TABLE IF NOT EXISTS notification (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    label text NOT NULL,
    topic text NOT NULL,
    payload jsonb NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT now() NOT NULL
)
