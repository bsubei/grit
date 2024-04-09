use sha1_smol::Digest;

// An object that can be stored in the database.
pub trait Object {
    fn get_oid(&self) -> &Digest;
    fn get_content(&self) -> &[u8];
}
