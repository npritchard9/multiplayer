#[derive(Debug)]
pub enum State {
    Waiting,
    Ready,
}

impl State {
    fn is_ready() {
        println!("GAME IS READY");
    }

    fn is_waiting() {
        println!("WAITING FOR OPPONENT");
    }

    pub fn game_status(&self) {
        match self {
            State::Waiting => State::is_waiting(),
            State::Ready => State::is_ready(),
        }
    }
}
