// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.

// Everything runs in test mode
#![cfg(test)]

mod checkin;

macro_rules! unit_test {
  ($($id:ident,)*) => {
    mod unit {
      $(
        #[test]
        fn $id() {
          $crate::checkin::runner::run_unit_test(stringify!($id));
        }
      )*
    }
  };
}

macro_rules! system_test {
  ($($id:ident,)*) => {
    mod system {
      $(
        #[test]
        fn $id() {
          $crate::checkin::runner::run_system_test(stringify!($id));
        }
      )*
    }
  };
}

unit_test!(microtask_test, test_test,);

system_test!(smoke_test,);
