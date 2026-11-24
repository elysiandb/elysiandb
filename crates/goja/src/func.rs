#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ResultType {
    Normal = 0,
    Yield = 1,
    YieldRes = 2,
    YieldDelegate = 3,
    YieldDelegateRes = 4,
    Await = 5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum YieldMarker {
    Await,
    Yield,
    YieldRes,
    YieldDelegate,
    YieldDelegateRes,
    YieldEmpty,
}

impl YieldMarker {
    pub fn result_type(self) -> ResultType {
        match self {
            YieldMarker::Await => ResultType::Await,
            YieldMarker::Yield | YieldMarker::YieldEmpty => ResultType::Yield,
            YieldMarker::YieldRes => ResultType::YieldRes,
            YieldMarker::YieldDelegate => ResultType::YieldDelegate,
            YieldMarker::YieldDelegateRes => ResultType::YieldDelegateRes,
        }
    }
}
