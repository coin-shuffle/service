CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

CREATE TABLE IF NOT EXISTS rooms
(
    id    UUID  NOT NULL DEFAULT uuid_generate_v4 () PRIMARY KEY,

    value JSONB NOT NULL
);


CREATE TABLE IF NOT EXISTS participants
(
    utxo_id NUMERIC(78, 0) NOT NULL PRIMARY KEY,

    value   JSONB          NOT NULL
);


CREATE TABLE IF NOT EXISTS queues
(
    token        CHAR(42)      NOT NULL,
    amount       NUMERIC(78,0) NOT NULL,

    participants JSONB         NOT NULL,

    PRIMARY KEY (token, amount)
);
