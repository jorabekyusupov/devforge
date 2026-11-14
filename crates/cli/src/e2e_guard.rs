#[cfg(all(feature = "e2e", not(debug_assertions)))]
compile_error!("DevForge CLI e2e fixture composition is supported only in debug acceptance builds");
