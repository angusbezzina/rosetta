resource "google_sql_database_instance" "main" {
  name = "main"
}

resource "google_storage_bucket_iam_member" "reader" {
  bucket = "assets"
}
