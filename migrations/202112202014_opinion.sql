create table if not exists opinion(
    id integer primary key,
    statement_id integer not null,
    signer_id integer not null,
    date integer not null,
    valid integer not null,
    serial integer not null,
    certainty integer not null,
    signature text not null,
    comment text,
    unique(statement_id,signer_id,date,serial),
    foreign key(statement_id) references statement(id),
    foreign key(signer_id) references statement(id)
);