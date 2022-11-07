# PLAN

## Create a moc node (1-2 day)

* accept/connect TCP;
* pnet, multistream select;
* noise handshake as an initiator and as a responder;
* create yamux streams;
* send/receive predefined messages;

## Create reproducible integration tests (2 days)

* try many connection and many streams in each connection, reproduce decryption failure;
* reproduce too many opened files problem;
* reproduce database failure;
* test filters;
* test timestamp range;

## Fix database related issues (2 days)

## Refactor BPF recorder

* Store low-level events immediately on disk. Check if the decryption failure is not happening (2 days)
* Write a state machine (should use redux?), that decrypt and parse low-level events (2-3 days)
