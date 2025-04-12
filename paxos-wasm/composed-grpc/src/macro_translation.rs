#[macro_export]
macro_rules! wit_convert {
    ($($from:path => $to:path { $($field:ident),+ });* $(;)?) => {
        $(
            impl From<$from> for $to {
                fn from(value: $from) -> Self {
                    Self {
                        $($field: value.$field.into()),+
                    }
                }
            }
            impl From<$to> for $from {
                fn from(value: $to) -> Self {
                    Self {
                        $($field: value.$field.into()),+
                    }
                }
            }
        )*
    };
}

#[macro_export]
macro_rules! wit_convert_enum_unit {
    ($from:path => $to:path { $($variant:ident),+ $(,)? }) => {
        impl From<$from> for $to {
            fn from(value: $from) -> Self {
                match value {
                    $(
                        $from::$variant => $to::$variant,
                    )+
                }
            }
        }
        impl From<$to> for $from {
            fn from(value: $to) -> Self {
                match value {
                    $(
                        $to::$variant => $from::$variant,
                    )+
                }
            }
        }
    };
}
