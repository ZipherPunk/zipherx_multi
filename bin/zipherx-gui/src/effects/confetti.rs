//! Particle effects: confetti for mempool acceptance, fireworks for block confirmation.

use egui::{Color32, Painter, Pos2, Rect};

/// A single visual particle.
pub struct Particle {
    pub pos: [f32; 2],
    pub vel: [f32; 2],
    pub color: Color32,
    pub alpha: f32,
    pub lifetime: f32,
    pub age: f32,
    pub size: f32,
    pub is_circle: bool,
}

/// Spawn green confetti from a point (mempool acceptance).
pub fn spawn_confetti(x: f32, y: f32) -> Vec<Particle> {
    let mut particles = Vec::with_capacity(80);
    for _ in 0..80 {
        let angle = rand::random::<f32>() * std::f32::consts::TAU;
        let speed = 100.0 + rand::random::<f32>() * 300.0;
        let g = 180 + (rand::random::<u8>() % 76);
        particles.push(Particle {
            pos: [x, y],
            vel: [angle.cos() * speed, angle.sin() * speed],
            color: Color32::from_rgb(0, g, 40 + rand::random::<u8>() % 30),
            alpha: 1.0,
            lifetime: 1.0 + rand::random::<f32>() * 1.5,
            age: 0.0,
            size: 3.0 + rand::random::<f32>() * 4.0,
            is_circle: false, // rectangles
        });
    }
    particles
}

/// Spawn cyan confetti from a point (incoming receive).
pub fn spawn_receive_confetti(x: f32, y: f32) -> Vec<Particle> {
    let mut particles = Vec::with_capacity(60);
    for _ in 0..60 {
        let angle = rand::random::<f32>() * std::f32::consts::TAU;
        let speed = 80.0 + rand::random::<f32>() * 250.0;
        let b = 200 + (rand::random::<u8>() % 56);
        particles.push(Particle {
            pos: [x, y],
            vel: [angle.cos() * speed, angle.sin() * speed],
            color: Color32::from_rgb(0, 150 + rand::random::<u8>() % 60, b),
            alpha: 1.0,
            lifetime: 1.0 + rand::random::<f32>() * 1.2,
            age: 0.0,
            size: 3.0 + rand::random::<f32>() * 3.0,
            is_circle: false,
        });
    }
    particles
}

/// Spawn gold/cyan fireworks from a point (block confirmation).
pub fn spawn_fireworks(x: f32, y: f32) -> Vec<Particle> {
    let mut particles = Vec::with_capacity(120);
    for _ in 0..120 {
        let angle = rand::random::<f32>() * std::f32::consts::TAU;
        let speed = 200.0 + rand::random::<f32>() * 400.0;
        // Bias upward
        let vy = -(speed * 0.5 + rand::random::<f32>() * speed * 0.5);
        let vx = angle.cos() * speed * 0.7;
        let color = if rand::random::<bool>() {
            Color32::from_rgb(255, 215, 0) // gold
        } else {
            Color32::from_rgb(0, 204, 255) // cyan
        };
        particles.push(Particle {
            pos: [x, y],
            vel: [vx, vy],
            color,
            alpha: 1.0,
            lifetime: 0.8 + rand::random::<f32>() * 1.2,
            age: 0.0,
            size: 2.0 + rand::random::<f32>() * 3.0,
            is_circle: true,
        });
    }
    particles
}

/// Update physics and draw particles. Removes dead particles.
pub fn update_and_draw(particles: &mut Vec<Particle>, painter: &Painter, dt: f32) {
    const GRAVITY: f32 = 400.0;
    const DAMPING: f32 = 0.98;

    particles.retain_mut(|p| {
        p.age += dt;
        if p.age >= p.lifetime {
            return false;
        }

        // Physics
        p.vel[1] += GRAVITY * dt;
        p.vel[0] *= DAMPING;
        p.vel[1] *= DAMPING;
        p.pos[0] += p.vel[0] * dt;
        p.pos[1] += p.vel[1] * dt;

        // Fade out
        p.alpha = 1.0 - (p.age / p.lifetime);
        let a = (p.alpha * 255.0) as u8;
        let color = Color32::from_rgba_unmultiplied(p.color.r(), p.color.g(), p.color.b(), a);

        let center = Pos2::new(p.pos[0], p.pos[1]);
        if p.is_circle {
            painter.circle_filled(center, p.size, color);
        } else {
            let rect = Rect::from_center_size(center, egui::Vec2::splat(p.size));
            painter.rect_filled(rect, 0.0, color);
        }

        true
    });
}
