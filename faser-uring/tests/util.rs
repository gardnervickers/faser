use futures_core::Future;

pub fn with_test_env<U, F>(f: impl FnOnce() -> F) -> Result<U, Box<dyn std::error::Error>>
where
    F: Future<Output = Result<U, Box<dyn std::error::Error>>>,
{
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .is_test(true)
        .try_init();

    let builder = io_uring::IoUring::builder();
    let driver = faser_uring::Driver::new(builder, 32)?;
    let mut ex = faser_executor::LocalExecutor::new(driver);
    ex.block_on((f)())
}
