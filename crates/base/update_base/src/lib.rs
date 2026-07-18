pub fn update_base(name: &str) -> (Arc<Progress>, thread::JoinHandle<Vec<(u64, String)>>) {
    let config = read_base_info::load_log(&name).unwrap();
    let path = read_base_info::get_zst_path(&name).unwrap();

    let test: Vec<HashMap<String, Value>> = config.get("paths");
}
