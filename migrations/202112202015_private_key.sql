create table if not exists private_key(
    signer_id integer not null,
    key text not null,
    foreign key(signer_id) references statement(id)
);