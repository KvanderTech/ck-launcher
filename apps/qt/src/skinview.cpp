#include "skinview.h"
#include "emote.h"
#include <QMatrix4x4>
#include <algorithm>
#include <cmath>
#include <limits>

SkinView::SkinView(bool small, QWidget *parent) : QWidget(parent), compact(small) {
    setMinimumSize(small ? 100 : 180, small ? 150 : 260);
    setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Expanding);
    if (compact)
        setAttribute(Qt::WA_TransparentForMouseEvents);
    else
        setCursor(Qt::OpenHandCursor);
    clock.start();
    timer.setInterval(16);
    connect(&timer, &QTimer::timeout, this, [this] {
        if (animated && isVisible() && !window()->isMinimized())
            update();
    });
    if (!compact)
        timer.start();
}
void SkinView::setSkin(const QImage &image, bool narrow) {
    texture = image.convertToFormat(QImage::Format_ARGB32);
    slim = narrow;
    update();
}
void SkinView::setCape(const QImage &image) {
    cape = image.convertToFormat(QImage::Format_ARGB32);
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
    if (event->button() == Qt::LeftButton) {
        drag = event->pos();
        setCursor(Qt::ClosedHandCursor);
    }
}
void SkinView::mouseReleaseEvent(QMouseEvent *) {
    setCursor(Qt::OpenHandCursor);
}
void SkinView::mouseMoveEvent(QMouseEvent *event) {
    if (event->buttons() & Qt::LeftButton) {
        yaw += (event->pos().x() - drag.x()) * .6;
        drag = event->pos();
        update();
    }
}
void SkinView::paintEvent(QPaintEvent *) {
    QPainter painter(this);
    if (texture.isNull()) {
        if (!compact) {
            painter.setPen(QColor(139, 166, 190));
            painter.drawText(rect(), Qt::AlignCenter,
                             QString::fromUtf8("Добавьте PNG-скин\n64×64 или 64×32"));
        }
        return;
    }
    constexpr double pi = 3.14159265358979323846;
    static const QVector<EmoteClip> clips{
        EmoteClip::load(QStringLiteral(":/assets/emotes/yes.json")),
        EmoteClip::load(QStringLiteral(":/assets/emotes/wave.json")),
        EmoteClip::load(QStringLiteral(":/assets/emotes/bow.json")),
        EmoteClip::load(QStringLiteral(":/assets/emotes/extend-arms.json"))};
    const double t =
        compact || !animated ? 0 : (fixedTime >= 0 ? fixedTime : clock.elapsed() / 1000.0);
    const double local = std::fmod(t, 8.0) - 2.5;
    const auto &clip = clips[int(t / 8) % clips.size()];
    const auto smooth = [](double v) {
        v = qBound(0.0, v, 1.0);
        return v * v * (3 - 2 * v);
    };
    const double weight = compact || !animated || local < 0
                              ? 0
                              : smooth(local / .22) * (1 - smooth((local - clip.duration()) / .4));
    auto pose = [&](const char *part, const QVector3D &bind) {
        auto p = clip.sample(QString::fromLatin1(part),
                             qBound(0.0, local * 20, clip.duration() * 20), bind);
        p.position = bind + (p.position - bind) * float(weight);
        p.rotation *= float(weight);
        p.scale = QVector3D(1, 1, 1) + (p.scale - QVector3D(1, 1, 1)) * float(weight);
        p.bend *= float(weight);
        return p;
    };
    auto torsoPose = pose("torso", {});
    auto rootPose = pose("body", {});
    QMatrix4x4 root;
    root.translate(rootPose.position.x(), -rootPose.position.y(), -rootPose.position.z());
    root.translate(0, 16, 0);
    root.rotate(float(-rootPose.rotation.z() * 180 / pi), 0, 0, 1);
    root.rotate(float(-rootPose.rotation.y() * 180 / pi), 0, 1, 0);
    root.rotate(float(rootPose.rotation.x() * 180 / pi), 1, 0, 0);
    root.scale(rootPose.scale);
    root.translate(0, -16, 0);
    auto matrix = [&](const EmotePart &p) {
        QMatrix4x4 m;
        m.translate(p.position.x(), 24 - p.position.y(), -p.position.z());
        m.rotate(float(-p.rotation.z() * 180 / pi), 0, 0, 1);
        m.rotate(float(-p.rotation.y() * 180 / pi), 0, 1, 0);
        m.rotate(float(p.rotation.x() * 180 / pi), 1, 0, 0);
        m.scale(p.scale);
        return m;
    };
    // Supersampled, perspective-correct, depth-tested software rendering works on
    // both Qt 5/Windows 7 and Qt 6 without depending on a particular GPU driver.
    const double factor = qBound(2.0, devicePixelRatioF() * 1.5, 3.0);
    const int rw = qMax(1, qRound(width() * factor)), rh = qMax(1, qRound(height() * factor));
    QImage frame(rw, rh, QImage::Format_ARGB32_Premultiplied);
    frame.fill(Qt::transparent);
    QVector<float> depth(rw * rh, -std::numeric_limits<float>::infinity());
    QMatrix4x4 view;
    view.rotate(-6, 1, 0, 0);
    view.rotate(float(yaw), 0, 1, 0);
    const double scale =
        std::min(width() / (compact ? 17.0 : 29.0), height() / (compact ? 24.5 : 39.0)) * factor;
    const double center = compact ? 20.0 : 16;
    struct Vertex {
        double x, y, inv, u, v;
    };
    struct Face {
        QVector<Vertex> vertices;
        const QImage *image;
        double depth, shade;
    };
    QVector<Face> faces;
    auto box = [&](float w, float h, float d, int tx, int ty, QMatrix4x4 m, QVector3D offset,
                   const QImage &image, const EmotePart &part, float pivot, bool bendUpper,
                   bool upperBody, float expand = 0) {
        const float x = w / 2 + expand, top = h / 2 + expand, z = d / 2 + expand;
        const QVector<QVector<QVector3D>> planes{
            {{-x, top, z}, {x, top, z}, {x, -top, z}, {-x, -top, z}},
            {{x, top, -z}, {-x, top, -z}, {-x, -top, -z}, {x, -top, -z}},
            {{x, top, z}, {x, top, -z}, {x, -top, -z}, {x, -top, z}},
            {{-x, top, -z}, {-x, top, z}, {-x, -top, z}, {-x, -top, -z}},
            {{-x, top, -z}, {x, top, -z}, {x, top, z}, {-x, top, z}},
            {{-x, -top, z}, {x, -top, z}, {x, -top, -z}, {-x, -top, -z}}};
        const QVector<QRectF> uvs{{double(tx + d), double(ty + d), w, h},
                                  {double(tx + 2 * d + w), double(ty + d), w, h},
                                  {double(tx + d + w), double(ty + d), d, h},
                                  {double(tx), double(ty + d), d, h},
                                  {double(tx + d), double(ty), w, d},
                                  {double(tx + d + w), double(ty), w, d}};
        for (int side = 0; side < planes.size(); ++side) {
            const int slices = side < 4 && std::abs(part.bend) > .0001 ? int(h) : 1;
            const auto &plane = planes[side];
            for (int slice = 0; slice < slices; ++slice) {
                const float a = float(slice) / slices, b = float(slice + 1) / slices;
                QVector<QVector3D> points{
                    plane[0] * (1 - a) + plane[3] * a, plane[1] * (1 - a) + plane[2] * a,
                    plane[1] * (1 - b) + plane[2] * b, plane[0] * (1 - b) + plane[3] * b};
                QVector<QVector3D> world;
                for (auto &point : points) {
                    point += offset;
                    point = EmoteClip::bendVertex(point, pivot, part.bend, part.axis, bendUpper);
                    point = m.map(point);
                    if (upperBody)
                        point =
                            EmoteClip::bendVertex(point, 18, torsoPose.bend, torsoPose.axis, true);
                    point = view.map(root.map(point));
                    world.append(point);
                }
                const auto normal =
                    QVector3D::crossProduct(world[1] - world[0], world[2] - world[0]).normalized();
                const double shade =
                    qBound(.64,
                           .9 - std::abs(double(normal.x())) * .13 +
                               std::abs(double(normal.z())) * .08 - double(normal.y()) * .07,
                           1.0);
                const auto uv = uvs[side];
                const double ratio = image.width() / 64.0;
                QVector<QPointF> coords{{uv.left(), uv.top() + uv.height() * a},
                                        {uv.right(), uv.top() + uv.height() * a},
                                        {uv.right(), uv.top() + uv.height() * b},
                                        {uv.left(), uv.top() + uv.height() * b}};
                Face face{{}, &image, 0, shade};
                for (int i = 0; i < 4; ++i) {
                    const auto &point = world[i];
                    const double inv = 1.0 / (100.0 - point.z());
                    face.vertices.append({rw / 2.0 + point.x() * scale * 100 * inv,
                                          rh / 2.0 - (point.y() - center) * scale * 100 * inv, inv,
                                          coords[i].x() * ratio * inv,
                                          coords[i].y() * ratio * inv});
                    face.depth += point.z() / 4.0;
                }
                faces.append(face);
            }
        }
    };
    auto torso = matrix(torsoPose);
    box(8, 12, 4, 16, 16, torso, {0, -6, 0}, texture, torsoPose, -6, true, false);
    auto headPose = pose("head", {});
    headPose.rotation.setY(headPose.rotation.y() + float(std::sin(t * .7) * .04 * (1 - weight)));
    auto head = matrix(headPose);
    box(8, 8, 8, 0, 0, head, {0, 4, 0}, texture, {}, 0, false, true);
    box(8, 8, 8, 32, 0, head, {0, 4, 0}, texture, {}, 0, false, true, .25);
    const bool modern = texture.height() >= texture.width();
    const float arm = slim ? 3 : 4;
    auto rightPose = pose("rightArm", {-5, 2, 0}), leftPose = pose("leftArm", {5, 2, 0});
    rightPose.rotation.setZ(rightPose.rotation.z() +
                            float((.035 + std::sin(t * 1.5) * .015) * (1 - weight)));
    leftPose.rotation.setZ(leftPose.rotation.z() -
                           float((.035 + std::sin(t * 1.5) * .015) * (1 - weight)));
    auto right = matrix(rightPose), left = matrix(leftPose);
    if (!modern)
        left.scale(-1, 1, 1);
    box(arm, 12, 4, 40, 16, right, {-arm / 2 + 1, -4, 0}, texture, rightPose, -4, false, true);
    box(arm, 12, 4, modern ? 32 : 40, modern ? 48 : 16, left,
        {modern ? arm / 2 - 1 : -arm / 2 + 1, -4, 0}, texture, leftPose, -4, false, true);
    auto rightLeg = pose("rightLeg", {-1.9f, 12, .1f}), leftLeg = pose("leftLeg", {1.9f, 12, .1f});
    auto rleg = matrix(rightLeg), lleg = matrix(leftLeg);
    if (!modern)
        lleg.scale(-1, 1, 1);
    box(4, 12, 4, 0, 16, rleg, {0, -6, 0}, texture, rightLeg, -6, false, false);
    box(4, 12, 4, modern ? 16 : 0, modern ? 48 : 16, lleg, {0, -6, 0}, texture, leftLeg, -6, false,
        false);
    if (modern) {
        box(8, 12, 4, 16, 32, torso, {0, -6, 0}, texture, torsoPose, -6, true, false, .15);
        box(arm, 12, 4, 40, 32, right, {-arm / 2 + 1, -4, 0}, texture, rightPose, -4, false, true,
            .15);
        box(arm, 12, 4, 48, 48, left, {arm / 2 - 1, -4, 0}, texture, leftPose, -4, false, true,
            .15);
        box(4, 12, 4, 0, 32, rleg, {0, -6, 0}, texture, rightLeg, -6, false, false, .15);
        box(4, 12, 4, 0, 48, lleg, {0, -6, 0}, texture, leftLeg, -6, false, false, .15);
    }
    if (!cape.isNull()) {
        QMatrix4x4 cloak;
        cloak.translate(0, 23, -2.6);
        cloak.rotate(float(-7 - std::sin(t * 1.8) * 2), 1, 0, 0);
        box(10, 16, 1, 0, 0, cloak, {0, -8, 0}, cape, {}, 0, false, true, .03);
    }
    std::stable_sort(faces.begin(), faces.end(),
                     [](const Face &a, const Face &b) { return a.depth < b.depth; });
    auto triangle = [&](const Vertex &a, const Vertex &b, const Vertex &c, const Face &face) {
        const double area = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
        if (std::abs(area) < .0001)
            return;
        const int x0 = qMax(0, int(std::floor(std::min({a.x, b.x, c.x}))));
        const int x1 = qMin(rw - 1, int(std::ceil(std::max({a.x, b.x, c.x}))));
        const int y0 = qMax(0, int(std::floor(std::min({a.y, b.y, c.y}))));
        const int y1 = qMin(rh - 1, int(std::ceil(std::max({a.y, b.y, c.y}))));
        for (int y = y0; y <= y1; ++y) {
            auto *line = reinterpret_cast<QRgb *>(frame.scanLine(y));
            for (int x = x0; x <= x1; ++x) {
                const double wa =
                    ((b.y - c.y) * (x + .5 - c.x) + (c.x - b.x) * (y + .5 - c.y)) / area;
                const double wb =
                    ((c.y - a.y) * (x + .5 - c.x) + (a.x - c.x) * (y + .5 - c.y)) / area;
                const double wc = 1 - wa - wb;
                if (wa < -1e-7 || wb < -1e-7 || wc < -1e-7)
                    continue;
                const double inv = wa * a.inv + wb * b.inv + wc * c.inv;
                if (inv <= depth[y * rw + x])
                    continue;
                const int u =
                    qBound(0, int((wa * a.u + wb * b.u + wc * c.u) / inv), face.image->width() - 1);
                const int v = qBound(0, int((wa * a.v + wb * b.v + wc * c.v) / inv),
                                     face.image->height() - 1);
                const QRgb texel = face.image->pixel(u, v);
                const int alpha = qAlpha(texel);
                if (!alpha)
                    continue;
                depth[y * rw + x] = float(inv);
                const auto dst = line[x];
                const auto lit = [&](int color, int background) {
                    return qBound(0,
                                  int(color * face.shade * alpha / 255.0) +
                                      background * (255 - alpha) / 255,
                                  255);
                };
                line[x] =
                    qRgba(lit(qRed(texel), qRed(dst)), lit(qGreen(texel), qGreen(dst)),
                          lit(qBlue(texel), qBlue(dst)), alpha + qAlpha(dst) * (255 - alpha) / 255);
            }
        }
    };
    for (const auto &face : faces) {
        triangle(face.vertices[0], face.vertices[1], face.vertices[2], face);
        triangle(face.vertices[0], face.vertices[2], face.vertices[3], face);
    }
    painter.setRenderHint(QPainter::SmoothPixmapTransform);
    painter.drawImage(rect(), frame);
}
