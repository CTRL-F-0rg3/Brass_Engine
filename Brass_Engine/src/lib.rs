pub mod render;
pub mod ecs;
pub mod input;

// Render
pub use render::context::RenderContext;
pub use render::renderer2d::{Renderer2D, Sprite, Color};
pub use render::renderer3d::{Renderer3D, Mesh, GpuMesh, Camera3D, Material, DirectionalLight, Vertex3D};
pub use render::texture_manager::TextureManager;
pub use render::app::{run, AppConfig};

// ECS
pub use ecs::world::{World, Entity};
pub use ecs::components::{Transform, RigidBody, SpriteComp, Tag};
pub use ecs::script::{Script, ScriptComponent};
pub use ecs::systems::{script_system, physics_system, render_sync_system, cleanup_system};

// Input
pub use input::input::{Input, Key, MouseButton};

// Math
pub use glam::{Vec2, Vec3, Vec4, Mat4, Quat};