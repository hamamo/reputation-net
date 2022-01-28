/// Trait implementation for a repository.
/// For each persistent type T, we can define a type Id<T> for primary and foreign keys for that type,
/// and Persistent<T> for values of that type with their associated Id
use std::{
    fmt::{self, Display, Formatter},
    hash::{Hash, Hasher},
    marker::PhantomData,
    ops::Deref,
};

use async_trait::async_trait;

use sqlx::TypeInfo;

/// The PrimtiveId type, i64 for Sqlite
type PrimitiveId = i64;

/// The Id<T> type using PhantomData to reference the identified type
pub struct Id<T> {
    id: PrimitiveId,
    marker: PhantomData<T>,
}

/// A wrapper for persistent structures of type T with their associated Id<T>
#[derive(Debug)]
pub struct Persistent<T> {
    pub id: Id<T>,
    pub data: T,
}

/// A wrapper for the result of a persist() operation
#[derive(Debug)]
pub struct PersistResult<T> {
    pub id: Id<T>,
    pub inserted: bool,
    pub data: T,
}

pub trait RowType {
    const TABLE: &'static str;
    const COLUMNS: &'static str;
}

#[async_trait]
pub trait Repository<T> {
    /// RowType describes the format of a database row
    type RowType: RowType;

    /// FkType describes the foreign keys that go into a database row (those are normally not included in T)
    type FkType;

    /// persist a record (find if old, insert if new)
    async fn persist(&mut self, data: T) -> Result<PersistResult<T>, sqlx::Error>;

    /// transform a database row into a record
    fn row_to_record(row: Self::RowType) -> Persistent<T>;
}

#[async_trait]
pub trait Get<T> {
    /// RowType describes the format of a database row
    type RowType: RowType;

    /// retrieve an existing record by id
    async fn get(&self, id: Id<T>) -> Result<Option<Persistent<T>>, sqlx::Error>;

    /// get all records
    async fn get_all(&self) -> Result<Vec<Persistent<T>>, sqlx::Error>;
}

impl<T> Id<T> {
    pub fn new(id: PrimitiveId) -> Self {
        Self {
            id,
            marker: PhantomData,
        }
    }
    pub fn with(self, data: T) -> Persistent<T> {
        Persistent { id: self, data }
    }
}

impl<T> Display for Id<T> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        write!(f, "{}", self.id)
    }
}

impl<T> std::fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        write!(f, "<{}>", self.id)
    }
}

impl<T> Hash for Id<T> {
    fn hash<H: Hasher>(&self, hasher: &mut H) {
        self.id.hash(hasher)
    }
}

impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Eq for Id<T> {}

impl<T> PartialOrd for Id<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.id.partial_cmp(&other.id)
    }
}

impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        Self::new(self.id)
    }
}

impl<T> Copy for Id<T> {}

impl<T> From<PrimitiveId> for Id<T> {
    fn from(id: PrimitiveId) -> Self {
        Self::new(id)
    }
}

impl<T> From<Id<T>> for PrimitiveId {
    fn from(val: Id<T>) -> Self {
        val.id
    }
}

impl<'r, DB: sqlx::Database, T> sqlx::Decode<'r, DB> for Id<T>
where
    i64: sqlx::Decode<'r, DB>,
{
    fn decode(
        value: <DB as sqlx::database::HasValueRef<'r>>::ValueRef,
    ) -> Result<Id<T>, Box<dyn std::error::Error + 'static + Send + Sync>> {
        let value = <PrimitiveId as sqlx::Decode<DB>>::decode(value)?;
        Ok(Id::new(value))
    }
}

impl<'q, DB: sqlx::Database, T> sqlx::Encode<'q, DB> for Id<T>
where
    PrimitiveId: sqlx::Encode<'q, DB>,
{
    fn encode_by_ref(
        &self,
        buf: &mut <DB as sqlx::database::HasArguments<'q>>::ArgumentBuffer,
    ) -> sqlx::encode::IsNull {
        <PrimitiveId as sqlx::Encode<DB>>::encode(self.id, buf)
    }
}

impl<DB: sqlx::Database, T> sqlx::Type<DB> for Id<T>
where
    PrimitiveId: sqlx::Type<DB>,
{
    fn type_info() -> DB::TypeInfo {
        <PrimitiveId as sqlx::Type<DB>>::type_info()
    }

    fn compatible(ty: &DB::TypeInfo) -> bool {
        ty.name() == Self::type_info().name()
    }
}

impl<T> Deref for Persistent<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.data
    }
}

impl<T> PersistResult<T> {
    pub fn new(id: Id<T>, data: T) -> Self {
        Self {
            id,
            inserted: true,
            data,
        }
    }
    pub fn is_new(&self) -> bool {
        self.inserted
    }

    pub fn old(id: Id<T>, data: T) -> Self {
        Self {
            id,
            inserted: false,
            data,
        }
    }

    #[allow(dead_code)]
    pub fn is_old(&self) -> bool {
        !self.inserted
    }

    pub fn wording(&self) -> &str {
        if self.inserted {
            "new"
        } else {
            "old"
        }
    }
}

impl<T> Deref for PersistResult<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.data
    }
}
