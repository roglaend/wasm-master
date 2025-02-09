// use log::info;
// use std::cell::RefCell;

// wit_bindgen::generate!({
//     world: "learner-world",
// });

// use crate::exports::paxos::learner::learner::{Guest, GuestLearnerResource, LearnerState};

// struct MyLearner;

// impl Guest for MyLearner {
//     type LearnerResource = MyLearnerResource;
// }

// struct MyLearnerResource {
//     learned_value: RefCell<Option<String>>,
// }

// impl GuestLearnerResource for MyLearnerResource {
//     /// Constructor: Initialize the learner
//     fn new() -> Self {
//         Self {
//             learned_value: RefCell::new(None),
//         }
//     }

//     /// Get the current state of the learner
//     fn get_state(&self) -> LearnerState {
//         LearnerState {
//             learned_value: self.learned_value.borrow().clone(),
//         }
//     }

//     /// Learn an accepted value
//     fn learn(&self, value: String) {
//         self.learned_value.replace(Some(value.clone()));
//         info!("Learner: Learned value '{}'", value);
//     }
// }

// export!(MyLearner with_types_in self);
