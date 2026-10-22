//! 内置 Adapter 实现。
//!
//! 知乎/B站/微博 直采官方公开接口；RSSHub 作为聚合兜底。
//! 抖音/小红书不自采，通过 RSSHub 路由获取索引。

pub mod zhihu;
pub mod bilibili;
pub mod weibo;
pub mod rsshub;

pub use zhihu::ZhihuAdapter;
pub use bilibili::BilibiliAdapter;
pub use weibo::WeiboAdapter;
pub use rsshub::{default_routes, RsshubAdapter, RsshubRoute, RsshubRouteAdapter};
