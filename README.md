# Reputation-Net

This P2P network is designed to share information about network operators (abuse contacts, IP to ASN mappings etc.)
as well as information about sources of network abuse.

The primary pieces of information are so called `Statement`s which express something about `Entity`s.
`Statement`s can only be shared when one or more `Signer`s share their `Opinion` about such statements
(basically, a number expressing how much they agree or disagree with the statement,
a date and a validity timespan for which the opinion should be considered valid.)

`Opinion`s are signed using the private key of the `Signer`.

Users of this information must consider how much they trust each signer in their evaluation of statements.
In addition they need to define policies that determine how this information should affect the handling of e-mail messages or other interactions (web forms etc.).

### Note
There's a secp256k1 key used for unit testing. Since this key isn't used for anything but these tests, it is safe to have it in a public repository.