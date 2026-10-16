//! `test/…/test_cover.py` vectors: cache key, original-quality fetch, file
//! write and format dispatch.

mod common;

use common::*;

#[tokio::test]
async fn fetches_original_quality_and_caches() {
    let tmp = tempfile::tempdir().unwrap();
    let server = wiremock::MockServer::start().await;
    mount_bytes(&server, "/cover.jpg@.jpg", vec![9, 9, 9], 1).await;

    let client = mock_client();
    let covers = database::CoverDatabase::open(tmp.path().join("thumbnail.db")).unwrap();
    let mut task = database::TaskInfo::default();
    task.File.name = "V".to_owned();
    task.File.download_path = tmp.path().display().to_string();
    task.File.folder = "f".to_owned();
    task.Episode.cover = format!("{}/cover.jpg", server.uri());

    extras::cover::parse(&client, &covers, &mut task, &empty_store())
        .await
        .unwrap();

    assert_eq!(std::fs::read(tmp.path().join("f/V.jpg")).unwrap(), vec![9, 9, 9]);
    // 请求的是原图（@.jpg 装饰）
    let request = &server.received_requests().await.unwrap()[0];
    assert_eq!(request.url.path(), "/cover.jpg@.jpg");
    // 装饰部分在 path 上：/cover.jpg@.jpg
    // 写文件的同时累加计数
    assert_eq!(task.Download.total_size, 3);
    assert_eq!(task.Download.downloaded_size, 3);

    // 第二次命中缓存：不再发请求
    let mut task2 = task.clone();
    task2.Download.total_size = 0;
    task2.Download.downloaded_size = 0;
    extras::cover::parse(&client, &covers, &mut task2, &empty_store())
        .await
        .unwrap();
    assert_eq!(server.received_requests().await.unwrap().len(), 1);

}

#[tokio::test]
async fn cover_type_png_writes_png() {
    let tmp = tempfile::tempdir().unwrap();
    let server = wiremock::MockServer::start().await;
    mount_bytes(&server, "/cover.png@.png", vec![1, 2], 1).await;

    let mut task = database::TaskInfo::default();
    task.File.name = "V".to_owned();
    task.File.download_path = tmp.path().display().to_string();
    task.File.folder = "f".to_owned();
    task.Episode.cover = format!("{}/cover.png", server.uri());
    task.Options.cover_type = Some("png".to_owned());

    let covers = database::CoverDatabase::open(tmp.path().join("thumbnail.db")).unwrap();
    extras::cover::parse(&mock_client(), &covers, &mut task, &empty_store())
        .await
        .unwrap();

    assert_eq!(std::fs::read(tmp.path().join("f/V.png")).unwrap(), vec![1, 2]);
}

#[tokio::test]
async fn retries_transport_failures() {
    let tmp = tempfile::tempdir().unwrap();
    // 端口关闭的地址：连接被拒，属于 transport 错误
    let client = mock_client();
    let mut task = database::TaskInfo::default();
    task.File.name = "V".to_owned();
    task.File.download_path = tmp.path().display().to_string();
    task.File.folder = "f".to_owned();
    task.Episode.cover = "http://127.0.0.1:1/cover.jpg".to_owned();

    let covers = database::CoverDatabase::open(tmp.path().join("thumbnail.db")).unwrap();
    let result = extras::cover::parse(&client, &covers, &mut task, &empty_store()).await;
    assert!(result.is_err());
    assert!(!tmp.path().join("f/V.jpg").exists());
}

#[tokio::test]
async fn options_override_global_format() {
    let tmp = tempfile::tempdir().unwrap();
    let server = wiremock::MockServer::start().await;
    mount_bytes(&server, "/cover.webp@.webp", vec![7], 1).await;

    let mut task = database::TaskInfo::default();
    task.File.name = "V".to_owned();
    task.File.download_path = tmp.path().display().to_string();
    task.File.folder = "f".to_owned();
    task.Episode.cover = format!("{}/cover.webp", server.uri());
    task.Options.cover_type = Some("webp".to_owned());

    let covers = database::CoverDatabase::open(tmp.path().join("thumbnail.db")).unwrap();
    extras::cover::parse(&mock_client(), &covers, &mut task, &empty_store())
        .await
        .unwrap();
    assert!(tmp.path().join("f/V.webp").exists());
}
