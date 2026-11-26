use std::future::Future;

/// A function accepted by a mutation hook, with either no arguments or one input.
pub trait MutationFunction<Args> {
    /// The value passed to `.dispatch`; `()` for a zero-argument function.
    type Input;
    /// The function's result type.
    type Output;
    /// The future returned by the function.
    type Fut: Future<Output = Self::Output> + 'static;

    /// Calls the function using the hook's dispatch input.
    fn invoke(&self, input: Self::Input) -> Self::Fut;
}

impl<F, Fut, O> MutationFunction<()> for F
where
    F: Fn() -> Fut,
    Fut: Future<Output = O> + 'static,
    O: 'static,
{
    type Input = ();
    type Output = O;
    type Fut = Fut;

    fn invoke(&self, (): Self::Input) -> Self::Fut {
        self()
    }
}

impl<F, I, Fut, O> MutationFunction<(I,)> for F
where
    F: Fn(I) -> Fut,
    I: 'static,
    Fut: Future<Output = O> + 'static,
    O: 'static,
{
    type Input = I;
    type Output = O;
    type Fut = Fut;

    fn invoke(&self, input: Self::Input) -> Self::Fut {
        self(input)
    }
}
