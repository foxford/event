alter table event
    drop constraint data_size;

alter table event
    add constraint data_size
        check (deleted_at IS NOT NULL OR octet_length(data::text) < 102400) not valid;
