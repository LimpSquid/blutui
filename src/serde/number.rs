use std::cmp::Ordering;
use std::fmt;
use std::ops::{Deref, DerefMut};

use serde::de::{Error, Visitor};
use serde::{Deserialize, Deserializer};

macro_rules! impl_str_number {
    ($name:ident, $number_ty:ty) => {
        #[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name($number_ty);

        impl Deref for $name {
            type Target = $number_ty;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl DerefMut for $name {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }

        impl PartialEq<$number_ty> for $name {
            fn eq(&self, other: &$number_ty) -> bool {
                self.0 == *other
            }
        }

        impl PartialOrd<$number_ty> for $name {
            fn partial_cmp(&self, other: &$number_ty) -> Option<Ordering> {
                self.0.partial_cmp(other)
            }
        }

        impl From<$number_ty> for $name {
            fn from(value: $number_ty) -> Self {
                Self(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct Impl;

                impl<'de> Visitor<'de> for Impl {
                    type Value = $name;

                    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                        f.write_str("number or number in string")
                    }

                    fn visit_u8<E: Error>(self, number: u8) -> Result<Self::Value, E> {
                        Ok($name(number.try_into().map_err(Error::custom)?))
                    }

                    fn visit_u16<E: Error>(self, number: u16) -> Result<Self::Value, E> {
                        Ok($name(number.try_into().map_err(Error::custom)?))
                    }

                    fn visit_u32<E: Error>(self, number: u32) -> Result<Self::Value, E> {
                        Ok($name(number.try_into().map_err(Error::custom)?))
                    }

                    fn visit_u64<E: Error>(self, number: u64) -> Result<Self::Value, E> {
                        Ok($name(number.try_into().map_err(Error::custom)?))
                    }

                    fn visit_i8<E: Error>(self, number: i8) -> Result<Self::Value, E> {
                        Ok($name(number.try_into().map_err(Error::custom)?))
                    }

                    fn visit_i16<E: Error>(self, number: i16) -> Result<Self::Value, E> {
                        Ok($name(number.try_into().map_err(Error::custom)?))
                    }

                    fn visit_i32<E: Error>(self, number: i32) -> Result<Self::Value, E> {
                        Ok($name(number.try_into().map_err(Error::custom)?))
                    }

                    fn visit_i64<E: Error>(self, number: i64) -> Result<Self::Value, E> {
                        Ok($name(number.try_into().map_err(Error::custom)?))
                    }

                    fn visit_str<E: Error>(self, number: &str) -> Result<Self::Value, E> {
                        number.parse().map($name).map_err(Error::custom)
                    }
                }

                deserializer.deserialize_any(Impl)
            }
        }
    };
}

impl_str_number! { StrU8,   u8 }
impl_str_number! { StrU16, u16 }
impl_str_number! { StrU32, u32 }
impl_str_number! { StrU64, u64 }
impl_str_number! { StrI8,   i8 }
impl_str_number! { StrI16, i16 }
impl_str_number! { StrI32, i32 }
impl_str_number! { StrI64, i64 }
