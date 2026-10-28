# axum-sea-orm-tx

Request-scoped [SeaORM] transactions for [axum].

Running a web request in a transaction makes requests atomic: mutations to the
database all get rolled back as one if there is any problem during the handling
of the request. A problem here is defined as a non-success, non-redirect
response.

It also lets you write tests that exercise the real database in multiple
requests. In this case, all the mutations in the transaction are rolled back
after the test is done, so that tests are isolated from each other.

The transaction is accessed in the handlers using the [`Tx`] Axum extractor.

In production, [`transaction`] begins each transaction on the database
connection. In tests, [`test_transaction`] begins each one as a savepoint in a
transaction the test holds. This way multiple test requests can be used to
observe changes to the database.

[SeaORM]: https://www.sea-ql.org/SeaORM/
[axum]: https://github.com/tokio-rs/axum
[`Tx`]: https://docs.rs/axum-sea-orm-tx/latest/axum_sea_orm_tx/struct.Tx.html
[`transaction`]: https://docs.rs/axum-sea-orm-tx/latest/axum_sea_orm_tx/fn.transaction.html
[`test_transaction`]: https://docs.rs/axum-sea-orm-tx/latest/axum_sea_orm_tx/fn.test_transaction.html
