pub mod render;
pub mod ecs;

pub use render::context::RenderContext;
pub use render::renderer2d::{Renderer2D, Sprite, Color};
pub use render::app::{run, AppConfig};

pub use ecs::world::{World, Entity};
pub use ecs::components::{Transform, RigidBody, SpriteComp, Tag};
pub use ecs::script::{Script, ScriptComponent};
pub use ecs::systems::{script_system, physics_system, render_sync_system, cleanup_system};

pub use glam::{Vec2, Vec4};