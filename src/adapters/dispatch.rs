use std::sync::Arc;

use crate::adapters::backend::factory::{BackendFactory, DefaultBackendFactory};
use crate::adapters::cli::Commands;
use crate::entities::common::Result;
use crate::entities::uri::Location;

/// 用默认 backend factory 跑命令；旧入口，等价于 `tidy_with(&DefaultBackendFactory, ...)`。
pub fn tidy(command: Commands) -> Result<()> {
    tidy_with(&DefaultBackendFactory, command)
}

/// 注入版入口：调用方提供 [`BackendFactory`]，常用于集成测试用 fake 装配混合 scheme。
pub fn tidy_with(factory: &dyn BackendFactory, command: Commands) -> Result<()> {
    match command {
        Commands::Copy {
            dry_run,
            include_non_media,
            sources,
            output,
        } => {
            let src_pairs = build_sources(factory, sources)?;
            let out_pair = build_source(factory, output)?;
            crate::usecases::copy(src_pairs, out_pair, dry_run, false, include_non_media)
        }
        Commands::Find {
            secure,
            sources,
            output,
        } => {
            let src_pairs = build_sources(factory, sources)?;
            let out_pair = output.map(|loc| build_source(factory, loc)).transpose()?;
            crate::usecases::find_duplicates(secure, src_pairs, out_pair)
        }
        Commands::Move {
            dry_run,
            include_non_media,
            sources,
            output,
        } => {
            let src_pairs = build_sources(factory, sources)?;
            let out_pair = build_source(factory, output)?;
            crate::usecases::copy(src_pairs, out_pair, dry_run, true, include_non_media)
        }
    }
}

fn build_source(factory: &dyn BackendFactory, loc: Location) -> Result<crate::usecases::Source> {
    let backend = factory.for_location(&loc)?;
    Ok((loc, backend))
}

fn build_sources(
    factory: &dyn BackendFactory,
    locs: Vec<Location>,
) -> Result<Vec<crate::usecases::Source>> {
    locs.into_iter()
        .map(|loc| build_source(factory, loc))
        .collect()
}
