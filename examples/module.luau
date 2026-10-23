local score = {
    Payload = "Round score",
    Total = 0,
}

function score:Add(points)
    self.Total = self.Total + points
    return self.Total
end

return score
