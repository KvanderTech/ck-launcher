#include "skinview.h"
#include <QMatrix4x4>
#include <algorithm>
#include <cmath>

SkinView::SkinView(bool small, QWidget *parent) : QWidget(parent), compact(small) {
    setMinimumSize(small ? 100 : 180, small ? 150 : 260);
    setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Expanding);
    if (compact)
        setAttribute(Qt::WA_TransparentForMouseEvents);
    else
        setCursor(Qt::OpenHandCursor);
    clock.start();
    timer.setInterval(40);
    connect(&timer, &QTimer::timeout, this, [this] {
        if (animated && isVisible() && !window()->isMinimized())
            update();
    });
    if (!compact)
        timer.start();
}
void SkinView::setSkin(const QImage &image, bool narrow) {
    texture = image;
    slim = narrow;
    update();
}
void SkinView::setCape(const QImage &image) {
    cape = image;
    update();
}
void SkinView::setAnimated(bool enabled) {
    animated = enabled;
    update();
}
void SkinView::setPoseTime(double seconds) {
    fixedTime = seconds;
    update();
}
void SkinView::mousePressEvent(QMouseEvent *event) {
    if (event->button() == Qt::LeftButton)
        drag = event->pos();
}
void SkinView::mouseMoveEvent(QMouseEvent *event) {
    if (event->buttons() & Qt::LeftButton) {
        yaw += (event->pos().x() - drag.x()) * .6;
        drag = event->pos();
        update();
    }
}
void SkinView::paintEvent(QPaintEvent *) {
    QPainter p(this);
    p.setRenderHint(QPainter::Antialiasing);
    p.setRenderHint(QPainter::SmoothPixmapTransform, false);
    if (texture.isNull()) {
        if (!compact) {
            p.setPen(QColor(139, 166, 190));
            p.drawText(rect(), Qt::AlignCenter,
                       QString::fromUtf8("Добавьте PNG-скин\n64×64 или 64×32"));
        }
        return;
    }
    const double t =
        compact || !animated ? 0 : (fixedTime >= 0 ? fixedTime : clock.elapsed() / 1000.0);
    struct Face {
        QPolygonF target;
        QRectF uv;
        const QImage *image;
        double depth, shade;
    };
    QVector<Face> faces;
    QMatrix4x4 view;
    view.rotate(-5, 1, 0, 0);
    view.rotate(float(yaw), 0, 1, 0);
    const double scale =
        std::min(width() / (compact ? 21.0 : 27.0), height() / (compact ? 24.0 : 37.0));
    const double center = compact ? 22.5 : 16.0;
    auto box = [&](float w, float h, float d, int tx, int ty, const QMatrix4x4 &local,
                   const QImage &image, float expand = 0) {
        QMatrix4x4 matrix = view * local;
        float x = w / 2 + expand, y = h / 2 + expand, z = d / 2 + expand;
        const QVector<QVector<QVector3D>> vertices = {
            {{-x, y, z}, {x, y, z}, {x, -y, z}, {-x, -y, z}},
            {{x, y, -z}, {-x, y, -z}, {-x, -y, -z}, {x, -y, -z}},
            {{x, y, z}, {x, y, -z}, {x, -y, -z}, {x, -y, z}},
            {{-x, y, -z}, {-x, y, z}, {-x, -y, z}, {-x, -y, -z}},
            {{-x, y, -z}, {x, y, -z}, {x, y, z}, {-x, y, z}},
            {{-x, -y, z}, {x, -y, z}, {x, -y, -z}, {-x, -y, -z}}};
        QVector<QRectF> uv = {{double(tx + d), double(ty + d), w, h},
                              {double(tx + 2 * d + w), double(ty + d), w, h},
                              {double(tx + d + w), double(ty + d), d, h},
                              {double(tx), double(ty + d), d, h},
                              {double(tx + d), double(ty), w, d},
                              {double(tx + d + w), double(ty), w, d}};
        for (int i = 0; i < vertices.size(); ++i) {
            QVector<QVector3D> world;
            QPolygonF screen;
            double depth = 0;
            for (auto v : vertices[i]) {
                v = matrix.map(v);
                world << v;
                depth += v.z();
                double projection = 95.0 / (95.0 - v.z());
                screen << QPointF(width() / 2.0 + v.x() * scale * projection,
                                  height() / 2.0 - (v.y() - center) * scale * projection);
            }
            auto normal =
                QVector3D::crossProduct(world[1] - world[0], world[2] - world[0]).normalized();
            double shade = qBound(.64,
                                  double(.77 + normal.y() * .09 - std::abs(normal.x()) * .1 +
                                         std::abs(normal.z()) * .18),
                                  1.0);
            double ratio = image.width() / 64.0;
            QRectF source(uv[i].x() * ratio, uv[i].y() * ratio, uv[i].width() * ratio,
                          uv[i].height() * ratio);
            faces.append({screen, source, &image, depth / 4, shade});
        }
    };
    // A calm idle loop periodically blends into the three bundled SPEmotes previews:
    // yes, extend-arms and bow. Drag rotation remains independent from the pose timeline.
    const double phase = std::fmod(t, 24.0);
    const auto pulse = [](double value, double begin, double end) {
        if (value <= begin || value >= end) return 0.0;
        return std::sin((value - begin) * 3.141592653589793 / (end - begin));
    };
    const double nod = pulse(phase, 6, 9);
    const double extend = pulse(phase, 11, 15);
    const double bow = pulse(phase, 17, 21);
    const double wave = pulse(phase, 2, 5);
    auto body = [&](float x, float y, float z, float rx = 0, float ry = 0, float rz = 0) {
        QMatrix4x4 m;
        m.translate(x, y, z);
        m.rotate(rx, 1, 0, 0);
        m.rotate(ry, 0, 1, 0);
        m.rotate(rz, 0, 0, 1);
        return m;
    };
    auto torso = body(0, 18 + float(std::sin(t * 1.8) * .08 - bow * 1.5), 0,
                      float(bow * 48));
    box(8, 12, 4, 16, 16, torso, texture);
    auto head = body(0, 24 - float(bow * 2), float(bow * 1.5),
                     float(std::sin(t * 1.4) * 3 + nod * std::sin(t * 15) * 18 + bow * 25),
                     float(std::sin(t * .8) * 5));
    head.translate(0, 4, 0);
    box(8, 8, 8, 0, 0, head, texture);
    box(8, 8, 8, 32, 0, head, texture, .25);
    const float arm = slim ? 3 : 4;
    auto right = body(-(4 + arm / 2), 22, 0, float(-wave * 145 - bow * 20), 0,
                      float(-3 - std::sin(t * 1.5) * 2 - wave * std::sin(t * 6) * 10 - extend * 88));
    right.translate(0, -4, 0);
    box(arm, 12, 4, 40, 16, right, texture);
    auto left =
        body(4 + arm / 2, 22, 0, float(std::sin(t * 1.6) * 2 - bow * 20), 0,
             float(3 + std::sin(t * 1.5) * 2 + extend * 88));
    left.translate(0, -4, 0);
    if (texture.height() < texture.width())
        left.scale(-1, 1, 1);
    box(arm, 12, 4, texture.height() >= texture.width() ? 32 : 40,
        texture.height() >= texture.width() ? 48 : 16, left, texture);
    auto rleg = body(-2, 12, 0, float(std::sin(t * 1.6) * 1.4));
    rleg.translate(0, -6, 0);
    box(4, 12, 4, 0, 16, rleg, texture);
    auto lleg = body(2, 12, 0, float(-std::sin(t * 1.6) * 1.4));
    lleg.translate(0, -6, 0);
    if (texture.height() < texture.width())
        lleg.scale(-1, 1, 1);
    box(4, 12, 4, texture.height() >= texture.width() ? 16 : 0,
        texture.height() >= texture.width() ? 48 : 16, lleg, texture);
    if (texture.height() >= texture.width()) {
        box(8, 12, 4, 16, 32, torso, texture, .15);
        box(arm, 12, 4, 40, 32, right, texture, .15);
        box(arm, 12, 4, 48, 48, left, texture, .15);
        box(4, 12, 4, 0, 32, rleg, texture, .15);
        box(4, 12, 4, 0, 48, lleg, texture, .15);
    }
    if (!cape.isNull()) {
        auto cloak = body(0, 23, -2.5, float(-7 - std::sin(t * 1.8) * 3));
        cloak.translate(0, -8, 0);
        box(10, 16, 1, 0, 0, cloak, cape, .03);
    }
    std::stable_sort(faces.begin(), faces.end(),
                     [](const Face &a, const Face &b) { return a.depth < b.depth; });
    for (const auto &face : faces) {
        if (!face.image->rect().contains(face.uv.toRect()))
            continue;
        auto fragment =
            face.image->copy(face.uv.toRect()).convertToFormat(QImage::Format_ARGB32_Premultiplied);
        {
            QPainter light(&fragment);
            light.setCompositionMode(QPainter::CompositionMode_SourceAtop);
            light.fillRect(fragment.rect(), QColor(0, 0, 0, qRound((1 - face.shade) * 255)));
        }
        const QRectF uv = fragment.rect();
        QPolygonF source(
            QVector<QPointF>{uv.topLeft(), uv.topRight(), uv.bottomRight(), uv.bottomLeft()});
        QTransform transform;
        if (!QTransform::quadToQuad(source, face.target, transform))
            continue;
        p.save();
        p.setTransform(transform);
        p.drawImage(uv, fragment, uv);
        p.restore();
    }
}
